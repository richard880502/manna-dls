//! 社群 endpoints (docs/COMMUNITY.md, docs/API.md「社群」). Transport only: the rules live in
//! `crate::community`. Every endpoint refuses (503) while the PI's `community_enabled` switch is
//! off.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::AuthTeacher;
use crate::community::{self, Page, Preview, Query, ShareRequest, ShareState};
use crate::error::{ApiJson, ApiPath, ApiQuery, AppResult};
use crate::AppState;

pub async fn explore(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(q): ApiQuery<Query>,
) -> AppResult<Json<Page>> {
    Ok(Json(community::explore(&state, teacher.id(), &q).await?))
}

pub async fn saved(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(q): ApiQuery<Query>,
) -> AppResult<Json<Page>> {
    Ok(Json(community::saved(&state, teacher.id(), &q).await?))
}

pub async fn mine(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(q): ApiQuery<Query>,
) -> AppResult<Json<community::MinePage>> {
    Ok(Json(community::mine(&state, teacher.id(), &q).await?))
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewQuery {
    pub level: Option<String>,
}

pub async fn preview(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiQuery(q): ApiQuery<PreviewQuery>,
) -> AppResult<Json<Preview>> {
    Ok(Json(
        community::preview(&state, teacher.id(), id, q.level.as_deref()).await?,
    ))
}

pub async fn picture(
    State(state): State<AppState>,
    _teacher: AuthTeacher,
    ApiPath((id, image_id)): ApiPath<(i64, i64)>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let (key, sha) = community::picture(&state, id, image_id).await?;
    crate::routes::worksheets::serve_image(&state, &key, &sha, &headers).await
}

pub async fn save(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Value>> {
    community::save(&state, teacher.id(), id).await?;
    Ok(Json(json!({"id": id, "saved": true})))
}

pub async fn unsave(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<Json<Value>> {
    community::unsave(&state, teacher.id(), id).await?;
    Ok(Json(json!({"id": id, "saved": false})))
}

pub async fn remix(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let copy = community::remix(&state, teacher.id(), id).await?;
    Ok((StatusCode::CREATED, Json(json!({"worksheet_id": copy}))))
}

/// `PUT /api/worksheets/{id}/share`: the owner shares, updates or withdraws.
pub async fn share(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(id): ApiPath<i64>,
    ApiJson(req): ApiJson<ShareRequest>,
) -> AppResult<Json<ShareState>> {
    Ok(Json(
        community::set_share(&state, teacher.id(), id, req).await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
}

pub async fn settings_get(
    State(state): State<AppState>,
    teacher: AuthTeacher,
) -> AppResult<Json<Value>> {
    teacher.require_pi()?;
    Ok(Json(
        json!({"enabled": crate::settings::community_enabled(&state.db).await?}),
    ))
}

/// `PUT /api/admin/community-settings` (PI): the research switch, logged like the memory switch.
pub async fn settings_put(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<Settings>,
) -> AppResult<Json<Value>> {
    teacher.require_pi()?;
    let mut tx = state.db.begin().await?;
    let value = json!(req.enabled);
    let previous = crate::settings::set(
        &mut tx,
        crate::settings::COMMUNITY_ENABLED,
        &value,
        teacher.id(),
    )
    .await?;
    crate::audit::log_usage(
        &mut *tx,
        teacher.id(),
        "setting_changed",
        json!({"key": crate::settings::COMMUNITY_ENABLED, "from": previous, "to": value}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"enabled": req.enabled})))
}
