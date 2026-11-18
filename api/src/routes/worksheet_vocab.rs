//! 詞彙級別檢查：處理超出級別的字 (docs/API.md「詞彙級別檢查：處理超出級別的字」): meanings
//! and replacements suggested for the chosen words of a check, and the teacher's choices applied
//! as one new worksheet version. Another teacher's worksheet or check is a 404.

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::auth::AuthTeacher;
use crate::error::{ApiJson, ApiPath, AppResult};
use crate::routes::worksheets::{state_of, WorksheetState};
use crate::worksheet::store;
use crate::worksheet::vocab;
use crate::worksheet::vocab_actions::{self, Applied, ApplyRequest, Suggestions};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct SuggestRequest {
    /// `gloss`（提供字義）or `replace`（換字）
    pub action: String,
    /// Base words of the check's `words` (at most 20).
    pub words: Vec<String>,
}

pub async fn suggest(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, check_id)): ApiPath<(i64, i64)>,
    ApiJson(req): ApiJson<SuggestRequest>,
) -> AppResult<Json<Suggestions>> {
    let ws = store::owned(&state.db, id, teacher.id()).await?;
    let check = vocab::get(&state.db, id, check_id).await?;
    let out =
        vocab_actions::suggest(&state, teacher.id(), &ws, &check, &req.action, &req.words).await?;
    Ok(Json(out))
}

/// 套用: what was done, the worksheet as it is now, and the version analysed again.
#[derive(Debug, Serialize)]
pub struct AppliedState {
    #[serde(flatten)]
    pub applied: Applied,
    pub worksheet: WorksheetState,
}

pub async fn apply(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((id, check_id)): ApiPath<(i64, i64)>,
    ApiJson(req): ApiJson<ApplyRequest>,
) -> AppResult<Json<AppliedState>> {
    let ws = store::owned(&state.db, id, teacher.id()).await?;
    let check = vocab::get(&state.db, id, check_id).await?;
    let applied = vocab_actions::apply(&state, teacher.id(), &ws, &check, req).await?;
    Ok(Json(AppliedState {
        applied,
        worksheet: state_of(&state, id).await?,
    }))
}
