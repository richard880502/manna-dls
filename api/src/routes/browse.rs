//! Browsing the library areas (docs/API.md「Phase 2.9」): `GET /api/browse/materials`
//! (教材參考 | 我的教材) and `GET /api/browse/templates` (平台範本 | 我的範本). Every logged-in
//! teacher; personal rows are only ever the caller's own.

use axum::extract::State;
use axum::Json;

use crate::auth::{AuthTeacher, Capability};
use crate::error::{ApiQuery, AppResult};
use crate::materials::browse::{
    self, MaterialCard, MaterialFacets, MaterialQuery, Page, Params, TemplateCard, TemplateFacets,
    TemplateQuery,
};
use crate::worksheet::template::extract::stale_after;
use crate::worksheet::template::store as tpl_store;
use crate::AppState;

pub async fn materials(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(pairs): ApiQuery<Vec<(String, String)>>,
) -> AppResult<Json<Page<MaterialCard, MaterialFacets>>> {
    teacher.require(Capability::UseMaterials)?;
    let query = MaterialQuery::teacher(&Params(pairs), teacher.id())?;
    Ok(Json(browse::materials(&state.db, &query).await?))
}

pub async fn templates(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiQuery(pairs): ApiQuery<Vec<(String, String)>>,
) -> AppResult<Json<Page<TemplateCard, TemplateFacets>>> {
    let query = TemplateQuery::teacher(&Params(pairs), teacher.id())?;
    // An extraction that died with the server shows as failed, not as extracting forever.
    tpl_store::expire_stale(&state.db, teacher.id(), stale_after(&state)).await?;
    Ok(Json(browse::templates(&state.db, &query).await?))
}
