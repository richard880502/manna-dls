//! Durable index leases, publication, retries and graph garbage collection.
use super::super::{POLICY_VERSION, SIDECAR_API_VERSION};
use super::client::SidecarClient;
use super::{compatible_sidecar, forget_health};
use crate::auth::{new_token, sha256_hex};
use crate::error::{AppError, AppResult};
use crate::AppState;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// Grace period before a replaced group is deleted, so searches already under way finish.
const REPLACED_GROUP_GRACE: &str = "2 minutes";
/// How often the worker lists the sidecar's groups to collect ones nothing refers to.
const SWEEP_EVERY: Duration = Duration::from_secs(600);
pub fn spawn_worker(state: AppState) {
    if !state.config.memory.enabled() {
        return;
    }
    tokio::spawn(async move {
        match reconcile(&state).await {
            Ok(0) => {}
            Ok(n) => tracing::info!(teachers = n, "queued memory index rebuilds after restart"),
            Err(e) => tracing::warn!(error = %e, "could not reconcile the memory index"),
        }
        loop {
            let work = {
                let state = state.clone();
                async move { sync_one(&state).await }
            };
            match crate::jobs::run("memory_index", 0, work).await {
                Ok(true) => continue,
                Ok(false) => {}
                Err(e) => tracing::warn!(error = %e, "memory index worker failed; retrying"),
            }
            let due = {
                let mut last = state.memory.last_sweep.lock().expect("sweep clock");
                let due = last.is_none_or(|t| t.elapsed() >= SWEEP_EVERY);
                if due {
                    *last = Some(Instant::now());
                }
                due
            };
            if due && compatible_sidecar(&state).await.is_some() {
                if let Err(e) = sweep_orphans(&state).await {
                    tracing::warn!(error = %e, "could not list memory index groups");
                }
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });
}

/// Queue every teacher whose habits changed after the published snapshot was read (changes
/// made while the index was not configured are not queued then) or who has none yet.
pub async fn reconcile(state: &AppState) -> sqlx::Result<u64> {
    if !state.config.memory.enabled() {
        return Ok(0);
    }
    Ok(sqlx::query(
        "INSERT INTO memory_indexes (teacher_id)
        SELECT teacher_id FROM teacher_memories GROUP BY teacher_id
        ON CONFLICT (teacher_id) DO UPDATE SET revision = memory_indexes.revision + 1,
            attempts = 0, next_attempt_at = now(), updated_at = now()
        WHERE memory_indexes.revision = memory_indexes.indexed_revision
          AND (memory_indexes.indexed_snapshot_at IS NULL OR memory_indexes.indexed_snapshot_at <
              (SELECT max(greatest(t.valid_at, coalesce(t.invalid_at, t.valid_at)))
                 FROM teacher_memories t WHERE t.teacher_id = memory_indexes.teacher_id))",
    )
    .execute(&state.db)
    .await?
    .rows_affected())
}

#[derive(sqlx::FromRow)]
struct Work {
    teacher_id: i64,
    revision: i64,
    group_id: Option<String>,
}

/// Claim a renewable durable job. Each attempt uses a distinct group, so a timed-out
/// sidecar can never write into the next attempt's graph or publish a stale revision.
pub async fn sync_one(state: &AppState) -> AppResult<bool> {
    state.ai.pinned(sync_pinned(state)).await
}

async fn sync_pinned(state: &AppState) -> AppResult<bool> {
    if !state.config.memory.enabled() || !super::providers_ready(state) {
        return Ok(false);
    }
    // Incompatible/offline sidecars leave the durable queue untouched. The next
    // poll recovers automatically when a compatible deployment returns.
    let Some(sidecar) = compatible_sidecar(state).await else {
        return Ok(false);
    };
    collect_garbage(state).await?;
    let token = new_token();
    // Inside a pinned job the settings are the job's; the forwarder reads them from here.
    let _pin = state.memory.pin_job(&token, state.ai.active());
    let work: Option<Work> = sqlx::query_as(
        "UPDATE memory_indexes SET lease_until=clock_timestamp()+interval '10 minutes',
        leased_at=clock_timestamp(),token_hash=$1,leased_revision=revision,
        leased_graphiti_version=$2,job_calls=0
        WHERE teacher_id=(SELECT teacher_id FROM memory_indexes
            WHERE revision>indexed_revision AND next_attempt_at<=now()
            AND (lease_until IS NULL OR lease_until<clock_timestamp())
            ORDER BY next_attempt_at,teacher_id FOR UPDATE SKIP LOCKED LIMIT 1)
        RETURNING teacher_id,revision,group_id",
    )
    .bind(sha256_hex(&token))
    .bind(&sidecar.graphiti_version)
    .fetch_optional(&state.db)
    .await?;
    let Some(work) = work else {
        return Ok(false);
    };
    let group = format!(
        "manna_t{}_r{}_{}",
        work.teacher_id,
        work.revision,
        &token[..12]
    );
    // Tracked before the sidecar creates it: an attempt that is never published is deleted
    // after its lease (10 min) and the HTTP limit (8 min) have certainly passed.
    sqlx::query(
        "INSERT INTO memory_index_garbage(group_id,not_before) VALUES($1,now()+interval '15 minutes')",
    )
    .bind(&group)
    .execute(&state.db)
    .await?;
    let active = super::super::list(&state.db, work.teacher_id, false).await?;
    let cfg = &state.config.memory;
    let result = SidecarClient::new(cfg)
        .index(&json!({
            "group_id":group,"memories":active,"proxy_url":cfg.proxy_url,
            "proxy_key":token,"model":state.ai.active().model,
            "embedding_model":state.config.embedding.model,
            "embedding_dims":state.config.embedding.dims,
            "policy_version":POLICY_VERSION,"api_version":SIDECAR_API_VERSION,
        }))
        .await;
    let success = match result {
        Ok(r) if r.status().is_success() => r.json::<Value>().await.ok().is_some_and(|v| {
            v["ok"] == true
                && v["api_version"] == SIDECAR_API_VERSION
                && v["graphiti_version"] == sidecar.graphiti_version
                && v["policy_version"] == POLICY_VERSION
        }),
        _ => false,
    };
    if success {
        let mut tx = state.db.begin().await?;
        // Compare-and-set: only the revision this job read, by this job, inside its lease.
        let published = sqlx::query(
            "UPDATE memory_indexes SET indexed_revision=$2,group_id=$3,
            indexed_graphiti_version=$5,indexed_snapshot_at=leased_at,attempts=0,
            lease_until=NULL,token_hash=NULL,last_error=NULL,updated_at=now()
            WHERE teacher_id=$1 AND revision=$2 AND token_hash=$4
              AND lease_until>clock_timestamp()",
        )
        .bind(work.teacher_id)
        .bind(work.revision)
        .bind(&group)
        .bind(sha256_hex(&token))
        .bind(&sidecar.graphiti_version)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;
        if published {
            sqlx::query("DELETE FROM memory_index_garbage WHERE group_id=$1")
                .bind(&group)
                .execute(&mut *tx)
                .await?;
            if let Some(old) = work.group_id {
                sqlx::query(&format!(
                    "INSERT INTO memory_index_garbage(group_id,not_before)
                    VALUES($1,now()+interval '{REPLACED_GROUP_GRACE}') ON CONFLICT DO NOTHING"
                ))
                .bind(old)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        if !published {
            // A newer change arrived meanwhile: the attempt group stays queued for deletion.
            release(state, work.teacher_id, &token, false).await?;
        }
    } else {
        forget_health(state).await;
        release(state, work.teacher_id, &token, true).await?;
    }
    Ok(true)
}

async fn release(state: &AppState, teacher_id: i64, token: &str, failed: bool) -> sqlx::Result<()> {
    sqlx::query("UPDATE memory_indexes SET lease_until=NULL,token_hash=NULL,
        attempts=attempts+$3::int, last_error=CASE WHEN $3 THEN 'Graphiti 同步未完成，稍後重試。' ELSE NULL END,
        next_attempt_at=CASE WHEN $3 THEN now()+make_interval(secs=>LEAST(3600,10*power(2,LEAST(attempts,8)))::int) ELSE now() END
        WHERE teacher_id=$1 AND token_hash=$2")
        .bind(teacher_id).bind(sha256_hex(token)).bind(failed).execute(&state.db).await?;
    Ok(())
}

/// Deletes queued groups whose time has come. The published group of any teacher is never
/// deleted, even if it was queued by mistake.
pub async fn collect_garbage(state: &AppState) -> AppResult<()> {
    let groups: Vec<String> = sqlx::query_scalar("SELECT group_id FROM memory_index_garbage g
        WHERE not_before<=now() AND NOT EXISTS(SELECT 1 FROM memory_indexes i WHERE i.group_id=g.group_id)
        ORDER BY not_before LIMIT 5").fetch_all(&state.db).await?;
    let cfg = &state.config.memory;
    for group in groups {
        if SidecarClient::new(cfg).delete_group(&group).await.is_ok() {
            sqlx::query("DELETE FROM memory_index_garbage WHERE group_id=$1")
                .bind(group)
                .execute(&state.db)
                .await?;
        }
    }
    Ok(())
}

fn is_group_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("manna_t") else {
        return false;
    };
    let mut parts = rest.splitn(2, "_r");
    let (Some(teacher), Some(rest)) = (parts.next(), parts.next()) else {
        return false;
    };
    let Some((revision, suffix)) = rest.split_once('_') else {
        return false;
    };
    !teacher.is_empty()
        && teacher.bytes().all(|b| b.is_ascii_digit())
        && !revision.is_empty()
        && revision.bytes().all(|b| b.is_ascii_digit())
        && suffix.len() == 12
        && suffix
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Queues groups that exist in FalkorDB but that nothing refers to (e.g. an empty graph left
/// by an older sidecar's search). In-flight attempts are already queued with their delay, the
/// published groups are skipped. Returns how many were newly queued.
pub async fn sweep_orphans(state: &AppState) -> AppResult<usize> {
    let cfg = &state.config.memory;
    if !cfg.enabled() {
        return Ok(0);
    }
    let listed = SidecarClient::new(cfg)
        .groups()
        .await
        .map_err(|e| AppError::Unavailable(format!("memory groups: {e}")))?;
    let mut queued = 0;
    for group in listed["groups"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|g| is_group_name(g))
    {
        queued += sqlx::query(
            "INSERT INTO memory_index_garbage(group_id,not_before)
            SELECT $1, now()+interval '15 minutes'
            WHERE NOT EXISTS(SELECT 1 FROM memory_indexes WHERE group_id=$1)
            ON CONFLICT DO NOTHING",
        )
        .bind(group)
        .execute(&state.db)
        .await?
        .rows_affected() as usize;
    }
    Ok(queued)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn group_names_are_recognised_exactly() {
        assert!(is_group_name("manna_t12_r3_0123456789ab"));
        for bad in [
            "manna_t12_r3_0123456789AB",
            "manna_t_r3_0123456789ab",
            "manna_t12_r3_0123456789a",
            "manna_memory",
            "other_t1_r1_0123456789ab",
        ] {
            assert!(!is_group_name(bad), "{bad}");
        }
    }
}
