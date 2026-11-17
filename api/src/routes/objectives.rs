//! `POST /api/objective-drafts` (docs/API.md「SIOP 目標」): Manna drafts 內容目標 and 語言目標
//! from the material when the teacher left both fields empty (G2), and shows only the drafts
//! that pass R1 and R3 (G3). The teacher's 採用／修改／改寫 is recorded with the existing
//! `POST /api/ai/:interaction_id/decision`.

use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use crate::auth::AuthTeacher;
use crate::error::{ApiJson, AppError, AppResult};
use crate::materials::{ocr, store as material_store};
use crate::objectives::draft::{self, DraftInput, DraftResult};
use crate::prep::is_bare_label;
use crate::routes::{clip_chars, log_usage, owned_class};
use crate::sop::{store as sop_store, MAX_MATERIAL_CHARS};
use crate::worksheet::material::SOURCE_LIBRARY;
use crate::worksheet::template::extract::{MSG_MATERIAL_NOT_FOUND, MSG_NO_TEXT};
use crate::worksheet::{Material, MAX_TITLE_CHARS};
use crate::AppState;

/// The tools a draft can be for (照我的學習單做 makes a `worksheet`).
pub const KINDS: [&str; 6] = [
    "comprehension",
    "tiered",
    "activity",
    "kit",
    "worksheet",
    "assessment",
];
const MSG_KIND: &str = "kind 需為 comprehension、tiered、activity、kit 或 worksheet。";
const MSG_NEEDS_MATERIAL: &str = "草擬目標需要教材：請上傳檔案、從教材庫選，或貼上課文。";
const MAX_REQUEST_CHARS: usize = 2000;
/// Lines of an earlier draft to write other than (「換一組」).
const AVOID_MAX: usize = 8;
const AVOID_CHARS: usize = 200;

#[derive(Debug, Default, Deserialize)]
pub struct PastedMaterial {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct DraftObjectives {
    pub kind: String,
    /// One of the teacher's (or a platform) library materials with text; wins over `material`.
    #[serde(default)]
    pub material_id: Option<i64>,
    /// Pasted text: used for the draft only, not stored.
    #[serde(default)]
    pub material: Option<PastedMaterial>,
    #[serde(default)]
    pub class_id: Option<i64>,
    /// 「教師的要求」; a bare chip or tool label is none.
    #[serde(default)]
    pub request: Option<String>,
    #[serde(default)]
    pub avoid: Vec<String>,
}

async fn material_of(
    db: &sqlx::PgPool,
    teacher_id: i64,
    req: &DraftObjectives,
) -> AppResult<Material> {
    if let Some(id) = req.material_id {
        let row = material_store::readable(db, id, teacher_id)
            .await
            .map_err(|_| AppError::bad_request(MSG_MATERIAL_NOT_FOUND))?;
        ocr::check_idle(&row)?;
        if row.text_status != "ready" || row.paragraphs.0.is_empty() {
            return Err(AppError::bad_request(MSG_NO_TEXT));
        }
        let stored = sop_store::Material::from(row);
        return Ok(Material::from_library(SOURCE_LIBRARY, None, &stored).0);
    }
    let pasted = req
        .material
        .as_ref()
        .ok_or_else(|| AppError::bad_request(MSG_NEEDS_MATERIAL))?;
    if pasted.text.chars().count() > MAX_MATERIAL_CHARS {
        return Err(AppError::bad_request(format!(
            "教材最多 {MAX_MATERIAL_CHARS} 字。"
        )));
    }
    let title = clip_chars(
        pasted.title.as_deref().unwrap_or_default().trim(),
        MAX_TITLE_CHARS - 1,
    );
    let material = Material::pasted(None, 0, &title, &pasted.text, "").0;
    if material.paragraphs.is_empty() {
        return Err(AppError::bad_request(MSG_NEEDS_MATERIAL));
    }
    Ok(material)
}

pub async fn draft(
    State(state): State<AppState>,
    teacher: AuthTeacher,
    ApiJson(req): ApiJson<DraftObjectives>,
) -> AppResult<Json<DraftResult>> {
    let db = &state.db;
    let kind = req.kind.trim().to_string();
    if !KINDS.contains(&kind.as_str()) {
        return Err(AppError::bad_request(MSG_KIND));
    }
    let request = req.request.as_deref().unwrap_or_default().trim();
    if request.chars().count() > MAX_REQUEST_CHARS {
        return Err(AppError::bad_request(format!(
            "內容最多 {MAX_REQUEST_CHARS} 字。"
        )));
    }
    let request = if is_bare_label(request) {
        String::new()
    } else {
        request.to_string()
    };
    if let Some(class_id) = req.class_id {
        owned_class(db, class_id, teacher.id()).await?;
    }
    let material = material_of(db, teacher.id(), &req).await?;
    let avoid: Vec<String> = req
        .avoid
        .iter()
        .map(|a| clip_chars(a.trim(), AVOID_CHARS))
        .filter(|a| !a.is_empty())
        .take(AVOID_MAX)
        .collect();
    let paragraphs = material.paragraphs.len();
    let result = draft::run(
        &state,
        DraftInput {
            teacher_id: teacher.id(),
            track: teacher.track().to_string(),
            kind: kind.clone(),
            material,
            class_id: req.class_id,
            request,
            avoid: avoid.clone(),
        },
    )
    .await?;
    log_usage(
        db,
        teacher.id(),
        "objectives_drafted",
        json!({
            "kind": kind, "material_id": req.material_id, "paragraphs": paragraphs,
            "class_id": req.class_id, "status": result.status,
            "interaction_id": result.interaction_id, "shown": result.objectives.len(),
            "hidden": result.hidden, "again": !avoid.is_empty(),
        }),
    )
    .await?;
    Ok(Json(result))
}
