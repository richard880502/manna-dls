//! Application-wide AI context enrichment followed by one traced provider attempt.
//! Feature-specific validators and repair policies live with their callers.

use serde_json::Value;

pub(crate) use crate::ai::execution::{
    finish_failure, finish_output, report_with, set_final, Attempt, Failure,
};
use crate::ai::pipeline::InteractionMeta;
use crate::ai::LlmRequest;
use crate::error::AppResult;
use crate::AppState;

pub(crate) async fn attempt(
    state: &AppState,
    meta: &InteractionMeta,
    snapshot: &Value,
    req: &LlmRequest,
    retry_of: Option<i64>,
) -> AppResult<Attempt> {
    let (snapshot, req) = crate::memory::augment(state, meta, snapshot, req).await?;
    let ai = state.ai.active();
    crate::ai::execution::attempt(&state.db, &ai, meta, &snapshot, &req, retry_of).await
}
