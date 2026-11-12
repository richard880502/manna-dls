//! The phone remote (手機遙控器): QR pairing and the run-scoped remote token (spec §4.3, §7.1;
//! docs/API.md「Phase 1.2」). A remote never logs in as the teacher: its token can only add
//! bookmarks to the paired run and undo its own, until the run ends (plus the late-sync window
//! for bookmarks tapped before the end).

use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::auth::{bearer_token, new_token, sha256_hex};
use crate::error::{ApiJson, AppError, AppResult};
use crate::record::{remote_token_alive, valid_device_type, Actor};
use crate::routes::runs::{run_events, sync_ops, RecordEvent, SyncRequest, SyncResponse};
use crate::AppState;

/// A request authenticated with a remote token.
#[derive(Debug, Clone, Copy)]
pub struct RemoteAuth {
    pub remote_id: i64,
    pub run_id: i64,
}

#[derive(sqlx::FromRow)]
struct RemoteRow {
    remote_id: i64,
    run_id: i64,
    ended_at: Option<DateTime<Utc>>,
}

impl FromRequestParts<AppState> for RemoteAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let expired =
            || AppError::Unauthorized("遙控器已失效，請在電腦上重新產生 QR Code 再掃描。".into());
        let token = bearer_token(&parts.headers).ok_or_else(expired)?;
        let row: RemoteRow = sqlx::query_as(
            "SELECT rr.id AS remote_id, rr.lesson_run_id AS run_id, r.ended_at
               FROM run_remotes rr
               JOIN lesson_runs r ON r.id = rr.lesson_run_id
               JOIN lessons l ON l.id = r.lesson_id
               JOIN units u ON u.id = l.unit_id
               JOIN teachers t ON t.id = u.teacher_id
              WHERE rr.token_hash = $1 AND rr.revoked_at IS NULL
                AND l.deleted_at IS NULL AND u.deleted_at IS NULL AND t.deleted_at IS NULL",
        )
        .bind(sha256_hex(&token))
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(expired)?;
        if !remote_token_alive(row.ended_at, Utc::now()) {
            return Err(AppError::Unauthorized(
                "本節課已結束，遙控器已失效。".into(),
            ));
        }
        sqlx::query("UPDATE run_remotes SET last_seen_at = now() WHERE id = $1")
            .bind(row.remote_id)
            .execute(&state.db)
            .await?;
        Ok(RemoteAuth {
            remote_id: row.remote_id,
            run_id: row.run_id,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct RemoteRun {
    pub run_id: i64,
    pub lesson_title: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    /// Only the bookmarks this remote created.
    pub events: Vec<RecordEvent>,
}

async fn remote_run(db: &PgPool, auth: RemoteAuth) -> AppResult<RemoteRun> {
    let (lesson_title, started_at, ended_at): (String, DateTime<Utc>, Option<DateTime<Utc>>) =
        sqlx::query_as(
            "SELECT l.title, r.started_at, r.ended_at
               FROM lesson_runs r JOIN lessons l ON l.id = r.lesson_id WHERE r.id = $1",
        )
        .bind(auth.run_id)
        .fetch_one(db)
        .await?;
    Ok(RemoteRun {
        run_id: auth.run_id,
        lesson_title,
        started_at,
        ended_at,
        events: run_events(db, auth.run_id, Some(auth.remote_id)).await?,
    })
}

#[derive(Debug, Deserialize)]
pub struct PairRequest {
    pub code: String,
    pub device_type: String,
}

#[derive(sqlx::FromRow)]
struct PairingRow {
    id: i64,
    run_id: i64,
    expired: bool,
    used: bool,
    revoked: bool,
    ended: bool,
}

/// Exchanges a one-time QR code for a run-scoped remote token.
pub async fn pair(
    State(state): State<AppState>,
    ApiJson(req): ApiJson<PairRequest>,
) -> AppResult<Json<Value>> {
    if !valid_device_type(&req.device_type) {
        return Err(AppError::bad_request("裝置類型不正確。"));
    }
    let code = req.code.trim();
    let invalid = || AppError::Unauthorized("這個 QR Code 無效，請在電腦上重新產生。".into());
    if code.is_empty() {
        return Err(invalid());
    }
    let mut tx = state.db.begin().await?;
    let p: PairingRow = sqlx::query_as(
        "SELECT p.id, p.lesson_run_id AS run_id, p.expires_at <= now() AS expired,
                p.used_at IS NOT NULL AS used, p.revoked_at IS NOT NULL AS revoked,
                r.ended_at IS NOT NULL AS ended
           FROM run_pairings p
           JOIN lesson_runs r ON r.id = p.lesson_run_id
           JOIN lessons l ON l.id = r.lesson_id
           JOIN units u ON u.id = l.unit_id
           JOIN teachers t ON t.id = u.teacher_id
          WHERE p.code_hash = $1
            AND l.deleted_at IS NULL AND u.deleted_at IS NULL AND t.deleted_at IS NULL
          FOR UPDATE OF p",
    )
    .bind(sha256_hex(code))
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(invalid)?;
    if p.revoked {
        return Err(AppError::Unauthorized(
            "這個 QR Code 已撤銷，請在電腦上重新產生。".into(),
        ));
    }
    if p.used {
        return Err(AppError::Unauthorized(
            "這個 QR Code 已經配對過了（每個只能用一次），請在電腦上重新產生。".into(),
        ));
    }
    if p.expired {
        return Err(AppError::Unauthorized(
            "這個 QR Code 已過期（10 分鐘內有效），請在電腦上重新產生。".into(),
        ));
    }
    if p.ended {
        return Err(AppError::Conflict(
            "本節課已結束，不能再配對遙控器。".into(),
        ));
    }
    sqlx::query("UPDATE run_pairings SET used_at = now() WHERE id = $1")
        .bind(p.id)
        .execute(&mut *tx)
        .await?;
    let token = new_token();
    let remote_id: i64 = sqlx::query_scalar(
        "INSERT INTO run_remotes (lesson_run_id, pairing_id, token_hash, device_type, last_seen_at)
         VALUES ($1, $2, $3, $4, now()) RETURNING id",
    )
    .bind(p.run_id)
    .bind(p.id)
    .bind(sha256_hex(&token))
    .bind(&req.device_type)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    let run = remote_run(
        &state.db,
        RemoteAuth {
            remote_id,
            run_id: p.run_id,
        },
    )
    .await?;
    Ok(Json(json!({ "token": token, "run": run })))
}

pub async fn get_run(
    State(state): State<AppState>,
    auth: RemoteAuth,
) -> AppResult<Json<RemoteRun>> {
    Ok(Json(remote_run(&state.db, auth).await?))
}

pub async fn sync(
    State(state): State<AppState>,
    auth: RemoteAuth,
    ApiJson(req): ApiJson<SyncRequest>,
) -> AppResult<Json<SyncResponse>> {
    let (results, ended_at) = sync_ops(
        &state.db,
        auth.run_id,
        Actor::Remote {
            remote_id: auth.remote_id,
        },
        &req.ops,
    )
    .await?;
    Ok(Json(SyncResponse {
        results,
        events: run_events(&state.db, auth.run_id, Some(auth.remote_id)).await?,
        ended_at,
    }))
}
