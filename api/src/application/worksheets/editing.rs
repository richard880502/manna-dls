//! Teacher-owned worksheet editing commands, independent of HTTP extraction and views.
use super::{
    check_pasted, check_template_output, checked_request, library_material, PastedMaterial,
};
use crate::audit::log_usage;
use crate::error::{AppError, AppResult};
use crate::materials::store::{self as material_store, LinkTarget};
use crate::serde_support::double_option;
use crate::sop::store as sop_store;
use crate::worksheet::ai as ws_ai;
use crate::worksheet::material::MSG_BAD_SELECTION;
use crate::worksheet::template::extract::{MSG_MATERIAL_NOT_FOUND, MSG_NO_TEXT};
use crate::worksheet::template::{prefill_settings, store as tpl_store};
use crate::worksheet::{
    auto_filename, auto_title, safe_filename, suggest_direct, Brief, Content, Material,
    SettingsPatch, GEN_OBJECTIVES, MAX_TITLE_CHARS,
};
use crate::worksheet::{images, store, thumbnail, vocab};
use crate::AppState;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
pub struct SaveWorksheet {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    /// The fields given replace the current ones (a full `WorksheetSettings` replaces them all).
    #[serde(default)]
    pub settings: Option<SettingsPatch>,
    #[serde(default)]
    pub brief: Option<Brief>,
    #[serde(default)]
    pub content: Option<Content>,
    /// 「教師的要求」; `null` or "" clears it.
    #[serde(default, deserialize_with = "double_option")]
    pub request: Option<Option<String>>,
    /// A template to follow (its current version; the settings are re-derived from it), or
    /// `null` to stop using one (the settings stay).
    #[serde(default, deserialize_with = "double_option")]
    pub template_id: Option<Option<i64>>,
    /// The version the editor last saw. With `content`: when a newer version exists and the
    /// content differs from it, nothing is saved (409, 「請重新載入」). Leaving it out saves as
    /// before (the last save wins).
    #[serde(default)]
    pub base_version_no: Option<i32>,
}

pub async fn save(state: &AppState, teacher_id: i64, id: i64, req: SaveWorksheet) -> AppResult<()> {
    let db = &state.db;
    let ws = store::owned(db, id, teacher_id).await?;
    let title = match req.title {
        Some(t) => {
            let t = t.trim().to_string();
            if t.chars().count() > MAX_TITLE_CHARS {
                return Err(AppError::bad_request(format!(
                    "標題最多 {MAX_TITLE_CHARS} 字。"
                )));
            }
            t
        }
        None => ws.title.clone(),
    };
    let filename = match req.filename {
        Some(f) if f.trim().is_empty() => auto_filename(&ws.material, None),
        Some(f) => safe_filename(&f),
        None => ws.filename.clone(),
    };
    let mut settings = match &req.settings {
        Some(patch) => patch
            .apply(&ws.settings)
            .normalised()
            .map_err(AppError::bad_request)?,
        None => ws.settings.clone(),
    };
    let template = match req.template_id {
        Some(Some(template_id)) => {
            let (row, version_no, content) =
                tpl_store::ready_for_use(db, teacher_id, template_id).await?;
            settings = prefill_settings(&settings, &content);
            Some(Some((row.id, version_no)))
        }
        Some(None) => Some(None),
        None => None,
    };
    let has_template = match template {
        Some(t) => t.is_some(),
        None => ws.template_id.is_some(),
    };
    check_template_output(has_template, &settings)?;
    let request = match &req.request {
        Some(r) => Some(checked_request(r.as_deref().unwrap_or_default())?),
        None => None,
    };
    if template.is_some() && ws_ai::jobs(state, id).await?.1 {
        return Err(AppError::Conflict(
            "Manna 正在產生這份學習單，好了之後再換版型。".to_string(),
        ));
    }
    let brief = req.brief.unwrap_or_else(|| ws.brief.clone());
    brief.validate().map_err(AppError::bad_request)?;
    let mut content = req.content;
    // G1: new 內容目標／語言目標 in the settings replace the worksheet's objectives from the two
    // fields (on the content sent, or the latest version); objectives given where there were
    // none are shown at the top (T1). Not while Manna is generating: that run keeps its own.
    let objectives_changed = settings.content_objective != ws.settings.content_objective
        || settings.language_objective != ws.settings.language_objective;
    if objectives_changed && !ws_ai::jobs(state, id).await?.1 {
        let had = !ws.settings.teacher_objectives().is_empty();
        if !had
            && !settings.teacher_objectives().is_empty()
            && !has_template
            && !settings.wants(GEN_OBJECTIVES)
        {
            settings.generation.push(GEN_OBJECTIVES.to_string());
            settings = settings.normalised().map_err(AppError::bad_request)?;
        }
        let mut base = match content.take() {
            Some(c) => c,
            None => store::latest(db, id).await?.content,
        };
        base.objectives = crate::objectives::synced(&base.objectives, &settings);
        content = Some(base);
    }
    if let Some(c) = &mut content {
        c.normalise().map_err(AppError::bad_request)?;
        let (_, running) = ws_ai::jobs(state, id).await?;
        if running {
            return Err(AppError::Conflict(
                "Manna 正在產生這份學習單，好了之後再改。".to_string(),
            ));
        }
        let lexicon = vocab::lexicon(db).await?;
        vocab::relookup_levels(c, lexicon.as_ref());
    }
    // The content first: a save over a newer version changes nothing at all.
    if let Some(c) = content {
        store::save_content_over(db, id, &c, "edit", req.base_version_no).await?;
    }
    store::update_fields(db, id, &title, &filename, &settings, &brief).await?;
    if let Some(r) = &request {
        store::set_request(db, id, r).await?;
    }
    if let Some(t) = template {
        store::set_template(db, id, t).await?;
    }
    Ok(())
}

pub async fn put_material(
    state: &AppState,
    teacher_id: i64,
    id: i64,
    req: PastedMaterial,
) -> AppResult<()> {
    let db = &state.db;
    let ws = store::owned(db, id, teacher_id).await?;
    check_pasted(&req)?;
    let (_, running) = ws_ai::jobs(state, id).await?;
    if running {
        return Err(AppError::Conflict(
            "Manna 正在產生這份學習單，好了之後再換教材。".to_string(),
        ));
    }
    let (material, seq) =
        library_material(db, teacher_id, Some(&ws.material), ws.paragraph_seq, &req).await?;
    if let Some(material_id) = material.source.material_id {
        material_store::link(db, material_id, LinkTarget::Worksheet(id)).await?;
    }
    // What to make (output, Bloom levels, counts, objective) is the teacher's, not a suggestion
    // from the material: it carries over.
    set_material_snapshot(db, &ws, &material, seq).await?;
    Ok(())
}

/// Said when the block was changed after the step that would be taken back.
pub const MSG_CHANGED_AFTER_STEP: &str =
    "這一區在採用之後又改過了，沒辦法直接復原到採用前；先把這一區改回採用後的樣子，再按復原。";

pub async fn undo_block(
    state: &AppState,
    teacher_id: i64,
    id: i64,
    block_id: &str,
) -> AppResult<()> {
    let db = &state.db;
    store::owned(db, id, teacher_id).await?;
    let (_, running) = ws_ai::jobs(state, id).await?;
    if running {
        return Err(AppError::Conflict(
            "Manna 正在產生這份學習單，好了之後再復原。".to_string(),
        ));
    }
    let step = store::peek_history(db, id, block_id)
        .await?
        .ok_or_else(|| AppError::Conflict("這個區塊沒有上一版可以復原。".to_string()))?;
    let picture_undo = images::undo::PictureUndo::from_json(step.picture_undo.as_ref());
    if step.after_block.is_some() {
        images::undo::check(db, id, &picture_undo).await?;
    }
    let mut tx = db.begin().await?;
    sqlx::query("SELECT id FROM worksheets WHERE id = $1 FOR UPDATE")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let latest: (serde_json::Value, i32) = sqlx::query_as(
        "SELECT content, version_no FROM worksheet_versions WHERE worksheet_id = $1
          ORDER BY version_no DESC LIMIT 1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let mut content: Content = serde_json::from_value(latest.0).unwrap_or_default();
    let Some(pos) = content.blocks.iter().position(|b| b.id == block_id) else {
        return Err(AppError::not_found("找不到這個區塊。"));
    };
    // A 採用 can only be taken back while the block is as it left it.
    if step
        .after_block
        .as_ref()
        .is_some_and(|a| *a != content.blocks[pos])
    {
        return Err(AppError::Conflict(MSG_CHANGED_AFTER_STEP.to_string()));
    }
    content.blocks[pos] = step.block.clone();
    content.normalise().map_err(AppError::bad_request)?;
    images::undo::revert(&mut tx, id, &picture_undo).await?;
    let saved = store::insert_version_from(
        &mut tx,
        id,
        &content,
        "undo",
        Some(json!({
            "cause": "undo", "history_id": step.id, "proposal_id": step.proposal_id,
            "interaction_id": step.interaction_id, "undone_version_no": step.version_no,
            "from_version_no": latest.1, "block_ids": [block_id],
        })),
    )
    .await?;
    let pictures = images::undo::snapshot(&mut *tx, id).await?;
    store::set_version_pictures(
        &mut *tx,
        id,
        saved.version_no,
        &images::undo::snapshot_json(&pictures),
    )
    .await?;
    store::mark_undone(&mut *tx, step.id, saved.version_no).await?;
    if let Some(proposal_id) = step.proposal_id {
        sqlx::query(
            "UPDATE worksheet_block_proposals SET undone_at = now()
              WHERE id = $1 AND worksheet_id = $2 AND undone_at IS NULL",
        )
        .bind(proposal_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    if let Some(interaction_id) = step.interaction_id {
        // Going back is the teacher turning the regenerated block down.
        sqlx::query(
            "INSERT INTO teacher_decisions (ai_interaction_id, teacher_id, item_index, decision, reason_note)
             VALUES ($1, $2, 0, 'reject', '學習單：復原上一版')",
        )
        .bind(interaction_id)
        .bind(teacher_id)
        .execute(&mut *tx)
        .await?;
    }
    log_usage(
        &mut *tx,
        teacher_id,
        "worksheet_block_undone",
        json!({
            "worksheet_id": id, "block_id": block_id, "version_no": saved.version_no,
            "from_version_no": latest.1, "undone_version_no": step.version_no,
            "proposal_id": step.proposal_id, "interaction_id": step.interaction_id,
            "pictures_reverted": picture_undo.slots.len(),
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn delete(state: &AppState, teacher_id: i64, id: i64) -> AppResult<()> {
    let ws = store::owned(&state.db, id, teacher_id).await?;
    store::soft_delete(&state.db, id).await?;
    // The kept preview goes too; a failure there never undoes or blocks the delete.
    thumbnail::forget(state, &ws).await;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct MaterialSelection {
    /// Paragraph ids of the library material to use; `null` or empty = all of them.
    #[serde(default)]
    pub paragraph_ids: Option<Vec<String>>,
}

/// Select a snapshot of the library material, keeping the teacher’s output settings.
pub async fn put_material_selection(
    state: &AppState,
    teacher_id: i64,
    id: i64,
    req: MaterialSelection,
) -> AppResult<()> {
    let db = &state.db;
    let ws = store::owned(db, id, teacher_id).await?;
    let (_, running) = ws_ai::jobs(state, id).await?;
    if running {
        return Err(AppError::Conflict(
            "Manna 正在產生這份學習單，好了之後再改要用的段落。".to_string(),
        ));
    }
    let Some(material_id) = ws.material.source.material_id else {
        return Err(AppError::bad_request(
            "這份學習單的教材不是從教材庫來的。請用「換教材」貼上要用的部分。",
        ));
    };
    let ids = crate::worksheet::material::clean_selection(
        req.paragraph_ids.as_deref().unwrap_or_default(),
    )
    .map_err(AppError::bad_request)?;
    let row = material_store::readable(db, material_id, teacher_id)
        .await
        .map_err(|_| AppError::bad_request(MSG_MATERIAL_NOT_FOUND))?;
    if row.text_status != "ready" || row.paragraphs.0.is_empty() {
        return Err(AppError::bad_request(MSG_NO_TEXT));
    }
    let stored = sop_store::Material::from(row);
    let kind = ws.material.source.kind.clone();
    let lesson_id = ws.material.source.lesson_id;
    let (material, seq) = match &ids {
        Some(ids) => Material::from_library_selection(&kind, lesson_id, &stored, ids)
            .ok_or_else(|| AppError::bad_request(MSG_BAD_SELECTION))?,
        None => Material::from_library(&kind, lesson_id, &stored),
    };
    set_material_snapshot(db, &ws, &material, seq.max(ws.paragraph_seq)).await?;
    log_usage(
        db,
        teacher_id,
        "worksheet_material_selection",
        json!({
            "worksheet_id": id, "material_id": material_id,
            "total": stored.paragraphs.len(), "selected": material.paragraphs.len(),
            "all": ids.is_none(),
        }),
    )
    .await?;
    Ok(())
}

/// Replacement and paragraph selection share how untouched settings follow new
/// suggestions, while teacher edits and custom names survive the snapshot change.
async fn set_material_snapshot(
    db: &sqlx::PgPool,
    ws: &store::Worksheet,
    material: &Material,
    seq: i32,
) -> AppResult<()> {
    let suggested = suggest_direct(material).with_focus_of(&ws.settings);
    // Selections the teacher never touched follow the new suggestion; her own stay.
    let settings = if ws.settings == ws.suggested {
        suggested.clone()
    } else {
        ws.settings.clone()
    };
    let title = if ws.title == auto_title(&ws.material) || ws.title.trim().is_empty() {
        auto_title(material)
    } else {
        ws.title.clone()
    };
    let filename = if ws.filename == auto_filename(&ws.material, None) {
        auto_filename(material, None)
    } else {
        ws.filename.clone()
    };
    store::set_material(
        db, ws.id, material, seq, &settings, &suggested, &title, &filename,
    )
    .await?;
    Ok(())
}
