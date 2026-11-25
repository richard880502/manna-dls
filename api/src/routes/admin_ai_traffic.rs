//! `GET /api/admin/ai-traffic` (PI only): how busy the model queue is now, what the last hour
//! looked like and a seven-day trend. Aggregates only: no teacher text and no per-teacher
//! figures. Contract: docs/API.md「AI 流量」.

use std::collections::BTreeMap;

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::auth::AuthTeacher;
use crate::error::AppResult;
use crate::AppState;

/// Rounds a percentile to one decimal so the JSON stays readable.
fn r1(v: Option<f64>) -> f64 {
    (v.unwrap_or(0.0) * 10.0).round() / 10.0
}

fn pair(p50: Option<f64>, p95: Option<f64>) -> Value {
    json!({ "p50": r1(p50), "p95": r1(p95) })
}

pub async fn get(State(state): State<AppState>, teacher: AuthTeacher) -> AppResult<Json<Value>> {
    teacher.require_pi()?;
    let db = &state.db;

    // Queue now: the partial indexes on ai_jobs cover queued and running rows.
    let (running, queued, oldest_wait): (i64, i64, Option<f64>) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE status = 'running'),
                count(*) FILTER (WHERE status = 'queued'),
                EXTRACT(EPOCH FROM now() - min(queued_at) FILTER (WHERE status = 'queued'))::float8
           FROM ai_jobs WHERE status IN ('queued', 'running')",
    )
    .fetch_one(db)
    .await?;

    // Jobs finished in the last hour, by status.
    let mut jobs: BTreeMap<String, i64> = ["done", "failed", "cancelled"]
        .iter()
        .map(|s| (s.to_string(), 0))
        .collect();
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT status, count(*) FROM ai_jobs
          WHERE finished_at >= now() - interval '1 hour' GROUP BY status",
    )
    .fetch_all(db)
    .await?;
    for (status, n) in rows {
        jobs.insert(status, n);
    }

    let (w50, w95): (Option<f64>, Option<f64>) = sqlx::query_as(
        "SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY w),
                percentile_cont(0.95) WITHIN GROUP (ORDER BY w)
           FROM (SELECT EXTRACT(EPOCH FROM started_at - queued_at)::float8 AS w FROM ai_jobs
                  WHERE finished_at >= now() - interval '1 hour' AND started_at IS NOT NULL) t",
    )
    .fetch_one(db)
    .await?;

    let run_rows: Vec<(String, Option<f64>, Option<f64>)> = sqlx::query_as(
        "SELECT kind,
                percentile_cont(0.5) WITHIN GROUP (ORDER BY r),
                percentile_cont(0.95) WITHIN GROUP (ORDER BY r)
           FROM (SELECT kind, EXTRACT(EPOCH FROM finished_at - started_at)::float8 AS r FROM ai_jobs
                  WHERE finished_at >= now() - interval '1 hour' AND started_at IS NOT NULL) t
          GROUP BY kind ORDER BY kind",
    )
    .fetch_all(db)
    .await?;
    let run_secs: serde_json::Map<String, Value> = run_rows
        .into_iter()
        .map(|(kind, p50, p95)| (kind, pair(p50, p95)))
        .collect();

    // Model calls in the last hour. `errors` is status error or timeout; the three kinds are
    // counted by the `error_detail` prefix and may overlap `errors`.
    #[allow(clippy::type_complexity)]
    let c: (
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
        Option<f64>,
        Option<f64>,
    ) = sqlx::query_as(
        "SELECT count(*),
                count(*) FILTER (WHERE status IN ('error', 'timeout')),
                count(*) FILTER (WHERE error_detail LIKE 'rate_limited:%'),
                count(*) FILTER (WHERE error_detail LIKE 'quota:%'),
                count(*) FILTER (WHERE error_detail LIKE 'overloaded:%'),
                COALESCE(sum((usage->>'prompt_tokens')::bigint), 0)::bigint,
                COALESCE(sum((usage->>'cached_tokens')::bigint), 0)::bigint,
                COALESCE(sum((usage->>'completion_tokens')::bigint), 0)::bigint,
                COALESCE(sum((usage->>'reasoning_tokens')::bigint), 0)::bigint,
                percentile_cont(0.5) WITHIN GROUP (ORDER BY latency_ms)::float8,
                percentile_cont(0.95) WITHIN GROUP (ORDER BY latency_ms)::float8
           FROM ai_interactions WHERE created_at >= now() - interval '1 hour'",
    )
    .fetch_one(db)
    .await?;

    // Seven Asia/Taipei dates ending today, zero-filled.
    let days: Vec<(String, i64, i64, i64, i64, i64)> = sqlx::query_as(
        "WITH d AS (
           SELECT g::date AS day FROM generate_series(
             (now() AT TIME ZONE 'Asia/Taipei')::date - 6,
             (now() AT TIME ZONE 'Asia/Taipei')::date, interval '1 day') g),
         i AS (
           SELECT (created_at AT TIME ZONE 'Asia/Taipei')::date AS day, count(*) AS calls,
                  COALESCE(sum((usage->>'prompt_tokens')::bigint), 0) AS p,
                  COALESCE(sum((usage->>'cached_tokens')::bigint), 0) AS c,
                  COALESCE(sum((usage->>'completion_tokens')::bigint), 0) AS o
             FROM ai_interactions
            WHERE created_at >= ((now() AT TIME ZONE 'Asia/Taipei')::date - 6)::timestamp
                                AT TIME ZONE 'Asia/Taipei'
            GROUP BY 1),
         j AS (
           SELECT (queued_at AT TIME ZONE 'Asia/Taipei')::date AS day, count(*) AS jobs
             FROM ai_jobs
            WHERE queued_at >= ((now() AT TIME ZONE 'Asia/Taipei')::date - 6)::timestamp
                               AT TIME ZONE 'Asia/Taipei'
            GROUP BY 1)
         SELECT to_char(d.day, 'YYYY-MM-DD'), COALESCE(i.calls, 0)::bigint, COALESCE(j.jobs, 0)::bigint,
                COALESCE(i.p, 0)::bigint, COALESCE(i.c, 0)::bigint, COALESCE(i.o, 0)::bigint
           FROM d LEFT JOIN i USING (day) LEFT JOIN j USING (day) ORDER BY d.day",
    )
    .fetch_all(db)
    .await?;
    let last_7_days: Vec<Value> = days
        .into_iter()
        .map(|(date, calls, jobs, p, c, o)| {
            json!({ "date": date, "calls": calls, "jobs": jobs,
                "prompt_tokens": p, "cached_tokens": c, "completion_tokens": o })
        })
        .collect();

    Ok(Json(json!({
        "now": {
            "max_running": state.config.ai_max_running_jobs,
            "running": running,
            "queued": queued,
            "oldest_wait_secs": oldest_wait.map_or(0, |s| s.max(0.0).floor() as i64),
            "queue_enabled": state.config.ai_queue_enabled,
            "quota_blocked_secs": state.ai.quota_blocked_secs(),
        },
        "last_hour": {
            "jobs": jobs,
            "wait_secs": pair(w50, w95),
            "run_secs": run_secs,
            "calls": c.0,
            "errors": c.1,
            "rate_limited": c.2,
            "quota": c.3,
            "overloaded": c.4,
            "tokens": { "prompt": c.5, "cached": c.6, "completion": c.7, "reasoning": c.8 },
            "latency_ms": pair(c.9, c.10),
        },
        "last_7_days": last_7_days,
    })))
}
