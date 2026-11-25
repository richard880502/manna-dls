//! 快照 (docs/API.md「快照」): a teacher's name on the latest version of a worksheet, so it can
//! be found again in the 版本紀錄. Removing a snapshot only removes the name; versions are never
//! deleted. Every worksheet belongs to one teacher; another teacher's is a 404.

use axum::extract::State;
use axum::Json;
use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::audit::log_usage;
use crate::auth::AuthTeacher;
use crate::error::{ApiJson, ApiPath, AppError, AppResult};
use crate::worksheet::{restore, store};
use crate::AppState;

pub const MAX_NAME_CHARS: usize = 60;
const MSG_STALE: &str = "這份學習單在你按下之後又有了新的版本，請再按一次「存成快照」。";
const MSG_HAS_SNAPSHOT: &str = "這一版已經是快照了，不用再存一次。";
const MSG_NAME_LONG: &str = "快照名稱最多 60 個字。";
const MSG_NAME_EMPTY: &str = "快照名稱不能是空的。";

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Snapshot {
    pub id: i64,
    pub version_no: i32,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRequest {
    pub name: Option<String>,
    /// The latest version the client has seen (after flushing its edits); anything else is a 409.
    pub base_version_no: i32,
}

#[derive(Debug, Deserialize)]
pub struct RenameRequest {
    pub name: String,
}

/// 「快照 11/25 17:30」 in Taipei time (no daylight saving, so a fixed +08:00 is exact).
pub fn default_name(now: DateTime<Utc>) -> String {
    let taipei = FixedOffset::east_opt(8 * 3600).expect("valid offset");
    format!("快照 {}", now.with_timezone(&taipei).format("%m/%d %H:%M"))
}

fn clean_name(raw: &str) -> AppResult<String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(AppError::bad_request(MSG_NAME_EMPTY));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(AppError::bad_request(MSG_NAME_LONG));
    }
    Ok(name.to_string())
}

/// `POST /api/worksheets/:id/snapshots`
pub async fn create(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<CreateRequest>,
) -> AppResult<Json<Snapshot>> {
    store::owned(&state.db, id, teacher.id()).await?;
    let named = req.name.as_deref().is_some_and(|n| !n.trim().is_empty());
    let name = if named {
        clean_name(req.name.as_deref().unwrap_or_default())?
    } else {
        default_name(Utc::now())
    };
    if let Some(why) = restore::busy(&state, id).await? {
        return Err(AppError::Conflict(why.to_string()));
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT id FROM worksheets WHERE id = $1 FOR UPDATE")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let latest = store::latest(&mut *tx, id).await?.version_no;
    if latest != req.base_version_no {
        return Err(AppError::Conflict(MSG_STALE.to_string()));
    }
    let snap: Option<Snapshot> = sqlx::query_as(
        "INSERT INTO worksheet_snapshots (worksheet_id, version_no, name) VALUES ($1, $2, $3)
         ON CONFLICT (worksheet_id, version_no) DO NOTHING
         RETURNING id, version_no, name, created_at",
    )
    .bind(id)
    .bind(latest)
    .bind(&name)
    .fetch_optional(&mut *tx)
    .await?;
    let snap = snap.ok_or_else(|| AppError::Conflict(MSG_HAS_SNAPSHOT.to_string()))?;
    log_usage(
        &mut *tx,
        teacher.id(),
        "worksheet_snapshot_saved",
        json!({ "worksheet_id": id, "version_no": snap.version_no, "named": named }),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(snap))
}

/// `PATCH /api/worksheets/:id/snapshots/:snapshot_id`
pub async fn rename(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, snapshot_id)): ApiPath<(i64, i64)>,
    ApiJson(req): ApiJson<RenameRequest>,
) -> AppResult<Json<Snapshot>> {
    store::owned(&state.db, id, teacher.id()).await?;
    let name = clean_name(&req.name)?;
    let mut tx = state.db.begin().await?;
    let snap: Snapshot = sqlx::query_as(
        "UPDATE worksheet_snapshots SET name = $3 WHERE id = $2 AND worksheet_id = $1
         RETURNING id, version_no, name, created_at",
    )
    .bind(id)
    .bind(snapshot_id)
    .bind(&name)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::not_found("找不到這個快照。"))?;
    log_usage(
        &mut *tx,
        teacher.id(),
        "worksheet_snapshot_renamed",
        json!({ "worksheet_id": id, "version_no": snap.version_no, "named": true }),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(snap))
}

/// `DELETE /api/worksheets/:id/snapshots/:snapshot_id`: removes the name only.
pub async fn remove(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, snapshot_id)): ApiPath<(i64, i64)>,
) -> AppResult<Json<serde_json::Value>> {
    store::owned(&state.db, id, teacher.id()).await?;
    let mut tx = state.db.begin().await?;
    let version_no: i32 = sqlx::query_scalar(
        "DELETE FROM worksheet_snapshots WHERE id = $2 AND worksheet_id = $1 RETURNING version_no",
    )
    .bind(id)
    .bind(snapshot_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::not_found("找不到這個快照。"))?;
    log_usage(
        &mut *tx,
        teacher.id(),
        "worksheet_snapshot_removed",
        json!({ "worksheet_id": id, "version_no": version_no, "named": true }),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}
