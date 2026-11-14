//! Unit materials (spec 4.5, minimal: pasted text) and the 教材優先 AI candidate focuses.
//!
//! Materials are owner-only and hard-deleted: DELETE removes the row and its chunks, and AI
//! input snapshots record only a hash of the text (never the text), so nothing of the
//! material survives a delete apart from the short verbatim excerpts inside AI outputs.

use std::collections::HashMap;

use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::ai::pipeline::{self, AiResult, ContextList, InteractionMeta};
use crate::ai::schemas::{self, MaterialFocusesOutput};
use crate::ai::LlmRequest;
use crate::auth::{self, AuthTeacher};
use crate::checks;
use crate::error::{ApiJson, ApiPath, AppError, AppResult};
use crate::preferences;
use crate::prompts;
use crate::routes::preferences::{preference_summary, snapshot as pref_snapshot};
use crate::routes::{log_usage, or_blank, owned_unit, resolved_profile, scaffold_id, track_label};
use crate::AppState;

pub const MAX_MATERIAL_CHARS: usize = 50_000;
pub const MAX_FILENAME_CHARS: usize = 200;
pub const DEFAULT_FILENAME: &str = "貼上的教材";
/// Target size of one stored chunk (characters).
pub const CHUNK_CHARS: usize = 1000;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct UnitMaterial {
    pub id: i64,
    pub filename: String,
    pub char_count: i32,
    pub created_at: DateTime<Utc>,
}

const MATERIAL_COLS: &str =
    "id, filename, COALESCE(char_length(text_content), 0)::int AS char_count, created_at";

pub async fn list(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(unit_id): ApiPath<i64>,
) -> AppResult<Json<Vec<UnitMaterial>>> {
    owned_unit(&state.db, unit_id, teacher.id()).await?;
    let rows = sqlx::query_as(&format!(
        "SELECT {MATERIAL_COLS} FROM unit_materials WHERE unit_id = $1 ORDER BY created_at, id"
    ))
    .bind(unit_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

/// Splits text into chunks of whole lines, each at most `max` characters where possible
/// (a single longer line is cut). Blank lines are dropped.
pub fn chunk_text(text: &str, max: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut cur = String::new();
    let mut cur_len = 0usize;
    for line in text
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
    {
        let mut line_chars: Vec<char> = line.chars().collect();
        while !line_chars.is_empty() {
            let room = max.saturating_sub(cur_len + usize::from(cur_len > 0));
            if cur_len > 0 && line_chars.len() > room {
                chunks.push(std::mem::take(&mut cur));
                cur_len = 0;
                continue;
            }
            if cur_len > 0 {
                cur.push('\n');
                cur_len += 1;
            }
            let take = line_chars.len().min(max);
            cur.extend(line_chars.drain(..take));
            cur_len += take;
        }
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

#[derive(Debug, Deserialize)]
pub struct CreateMaterial {
    pub text: String,
    #[serde(default)]
    pub filename: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(unit_id): ApiPath<i64>,
    ApiJson(req): ApiJson<CreateMaterial>,
) -> AppResult<Json<UnitMaterial>> {
    owned_unit(&state.db, unit_id, teacher.id()).await?;
    if req.text.trim().is_empty() {
        return Err(AppError::bad_request("請貼上教材文字。"));
    }
    let chars = req.text.chars().count();
    if chars > MAX_MATERIAL_CHARS {
        return Err(AppError::bad_request(format!(
            "教材文字最多 {MAX_MATERIAL_CHARS} 字（目前 {chars} 字），請分段貼上。"
        )));
    }
    let filename = req
        .filename
        .as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .unwrap_or(DEFAULT_FILENAME);
    if filename.chars().count() > MAX_FILENAME_CHARS {
        return Err(AppError::bad_request(format!(
            "名稱最多 {MAX_FILENAME_CHARS} 字。"
        )));
    }
    let mut tx = state.db.begin().await?;
    let material: UnitMaterial = sqlx::query_as(&format!(
        "INSERT INTO unit_materials (unit_id, filename, text_content) VALUES ($1, $2, $3)
         RETURNING {MATERIAL_COLS}"
    ))
    .bind(unit_id)
    .bind(filename)
    .bind(&req.text)
    .fetch_one(&mut *tx)
    .await?;
    let chunks = chunk_text(&req.text, CHUNK_CHARS);
    sqlx::query(
        "INSERT INTO unit_material_chunks (unit_material_id, text)
         SELECT $1, t FROM unnest($2::text[]) WITH ORDINALITY AS c(t, ord) ORDER BY ord",
    )
    .bind(material.id)
    .bind(&chunks)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(material))
}

/// Physically deletes the material and its chunks (spec 4.5, D5).
pub async fn remove(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath((unit_id, material_id)): ApiPath<(i64, i64)>,
) -> AppResult<Json<Value>> {
    owned_unit(&state.db, unit_id, teacher.id()).await?;
    let mut tx = state.db.begin().await?;
    let chars: Option<i32> = sqlx::query_scalar(
        "SELECT COALESCE(char_length(text_content), 0)::int FROM unit_materials
          WHERE id = $1 AND unit_id = $2 FOR UPDATE",
    )
    .bind(material_id)
    .bind(unit_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(chars) = chars else {
        return Err(AppError::not_found("找不到這份教材。"));
    };
    sqlx::query("DELETE FROM unit_material_chunks WHERE unit_material_id = $1")
        .bind(material_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM unit_materials WHERE id = $1")
        .bind(material_id)
        .execute(&mut *tx)
        .await?;
    // A trace that something was deleted (no content), so interactions that referenced the
    // material remain explainable.
    log_usage(
        &mut *tx,
        teacher.id(),
        "material_deleted",
        json!({ "unit_id": unit_id, "material_id": material_id, "char_count": chars }),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- 教材優先 candidates

#[derive(Debug, Deserialize)]
pub struct FocusesRequest {
    pub material_id: i64,
    /// The class the teacher is working in (Phase 1.4), optional.
    #[serde(default)]
    pub class_id: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct MaterialText {
    id: i64,
    filename: String,
    text_content: Option<String>,
}

pub async fn material_focuses(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiPath(unit_id): ApiPath<i64>,
    ApiJson(req): ApiJson<FocusesRequest>,
) -> AppResult<Json<AiResult<MaterialFocusesOutput>>> {
    let unit = owned_unit(&state.db, unit_id, teacher.id()).await?;
    let profile = resolved_profile(
        &state.db,
        teacher.id(),
        req.class_id,
        "",
        &unit.class_profile,
    )
    .await?;
    let material: MaterialText = sqlx::query_as(
        "SELECT id, filename, text_content FROM unit_materials WHERE id = $1 AND unit_id = $2",
    )
    .bind(req.material_id)
    .bind(unit_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("找不到這份教材。"))?;
    let text = material.text_content.unwrap_or_default();
    if text.trim().is_empty() {
        return Err(AppError::bad_request("這份教材沒有文字內容。"));
    }
    if !state.ai.enabled() {
        return Ok(Json(AiResult::disabled()));
    }
    let prompt = prompts::latest(&state.db, prompts::KEY_MATERIAL_FOCUSES)
        .await?
        .ok_or_else(|| {
            AppError::Internal(anyhow::anyhow!("no prompt version for material_focuses"))
        })?;
    let prefs = preferences::latest_answers(&state.db, teacher.id()).await?;
    let track = teacher.track().to_string();
    let class_profile = profile.text.clone();

    let mut vars: HashMap<&str, String> = HashMap::from([
        ("school_track", track_label(&track).to_string()),
        ("unit_title", unit.title.clone()),
        ("class_profile", or_blank(&class_profile)),
        ("detail_instruction", prefs.detail_instruction().to_string()),
        ("material_filename", material.filename.clone()),
        ("material_text", text.clone()),
    ]);
    let required = [
        ("class_profile", "班級特性"),
        ("detail_instruction", "回覆長度"),
        ("material_text", "教材"),
    ];
    let system = prompts::render(&prompt.system_text, &vars);
    let user = prompts::render_user_with(&prompt, &vars, &required);
    // The logged copy never contains the material text.
    vars.insert("material_text", prompts::redacted("教材原文", &text));
    let logged_user = prompts::render_user_with(&prompt, &vars, &required);
    let snapshot = json!({
        "inputs": vars,
        "material": {
            "material_id": material.id,
            "char_count": text.chars().count(),
            "sha256": auth::sha256_hex(&text),
        },
        "preferences": pref_snapshot(&prefs),
        "class_context": profile.snapshot(),
        "prompt": {
            "key": prompts::KEY_MATERIAL_FOCUSES, "version_no": prompt.version_no,
            "system": system, "user": logged_user
        },
        "schema": schemas::MATERIAL_FOCUSES_SCHEMA_NAME,
    });
    let meta = InteractionMeta {
        teacher_id: teacher.id(),
        module: "material_focuses",
        unit_id: Some(unit.id),
        lesson_id: None,
        scaffold_id: scaffold_id(&state.db, "SC-MATERIAL").await?,
        prompt_version_id: Some(prompt.id),
    };
    let first_id = pipeline::insert_interaction(
        &state.db,
        &meta,
        state.ai.provider_name(),
        &state.ai.model,
        &snapshot,
        1,
        None,
    )
    .await?;
    let llm_req = LlmRequest {
        system,
        user,
        schema_name: schemas::MATERIAL_FOCUSES_SCHEMA_NAME,
        schema: schemas::material_focuses_schema(),
        fake_context: json!({ "task": "material_focuses", "material_text": text }),
        attempt: 1,
        max_tokens: None,
        temperature: None,
    };
    let outcome = pipeline::run(
        &state,
        &meta,
        first_id,
        &snapshot,
        llm_req,
        |raw: &Value| {
            pipeline::parse_and_check::<MaterialFocusesOutput>(raw, |o| {
                (checks::check_material_focuses(o, &text), Vec::new())
            })
        },
    )
    .await?;

    let mut context = ContextList::default();
    context
        .add("教材摘錄", &format!("{}：{}", material.filename, text))
        .add("單元", &unit.title)
        .add(&profile.context_label(), &class_profile)
        .add("你的備課偏好", &preference_summary(&prefs, &[]));
    Ok(Json(
        outcome
            .into_result(|_| Vec::new())
            .with_context(context.into_vec()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_keep_lines_and_respect_the_limit() {
        let text = "第一段第一行\n第一段第二行\n\n第二段\n";
        assert_eq!(
            chunk_text(text, 1000),
            vec!["第一段第一行\n第一段第二行\n第二段"]
        );
        let chunks = chunk_text(text, 8);
        assert_eq!(chunks, vec!["第一段第一行", "第一段第二行", "第二段"]);
        let long = "a".repeat(25);
        let chunks = chunk_text(&long, 10);
        assert_eq!(
            chunks.iter().map(|c| c.len()).collect::<Vec<_>>(),
            vec![10, 10, 5]
        );
        assert!(chunk_text("  \n\n", 10).is_empty());
    }
}
