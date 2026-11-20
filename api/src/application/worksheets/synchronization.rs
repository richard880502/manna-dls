//! Teacher-confirmed synchronization between a worksheet and its lesson SOP.
use crate::audit::log_usage;
use crate::error::{AppError, AppResult};
use crate::sop::{self, store as sop_store};
use crate::teaching_context::{owned_lesson, owned_unit};
use crate::worksheet::store::{self, Worksheet};
use crate::worksheet::sync::{self, Preview};
use crate::worksheet::{Content, MAX_TITLE_CHARS};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;

pub(crate) async fn lesson_sop_diff(
    db: &PgPool,
    ws: &Worksheet,
    content: &Content,
) -> AppResult<Option<(i64, Option<sop_store::Version>, Vec<sync::Change>)>> {
    let Some(lesson_id) = ws.lesson_id else {
        return Ok(None);
    };
    // A lesson deleted since the link was made: the worksheet stays, unlinked in effect.
    if owned_lesson(db, lesson_id, ws.teacher_id).await.is_err() {
        return Ok(None);
    }
    let sop_version = sop_store::latest(db, lesson_id).await?;
    let sop_content = sop_version
        .as_ref()
        .map(|v| v.content.clone())
        .unwrap_or_default();
    let changes = sync::diff(content, &ws.material, &sop_content);
    Ok(Some((lesson_id, sop_version, changes)))
}

async fn write_back(
    db: &PgPool,
    ws: &Worksheet,
    lesson_id: i64,
    keys: &[String],
) -> AppResult<(i32, usize)> {
    let version = sop_store::latest(db, lesson_id).await?;
    let sop_material =
        sop_store::material(db, version.as_ref().and_then(|v| v.material_id)).await?;
    let mut sop_content = version.map(|v| v.content).unwrap_or_default();
    let mut content = store::latest(db, ws.id).await?.content;
    let applied = sync::apply(
        &mut content,
        &ws.material,
        &mut sop_content,
        sop_material
            .as_ref()
            .map(|m| m.paragraphs.as_slice())
            .unwrap_or(&[]),
        keys,
    );
    sop::validate(&sop_content).map_err(AppError::bad_request)?;
    let items = crate::curriculum::by_codes(db, &sop::linked_codes(&sop_content)).await?;
    sop::resolve_links(&mut sop_content, &items).map_err(|code| {
        AppError::bad_request(format!("課綱條目「{code}」不在知識庫裡，不能連結。"))
    })?;
    let saved = sop_store::save_content(db, lesson_id, &sop_content).await?;
    content.normalise().map_err(AppError::bad_request)?;
    store::save_content(db, ws.id, &content, "edit").await?;
    Ok((saved.version_no, applied))
}

#[derive(Debug, Deserialize)]
pub struct ApplySync {
    #[serde(default)]
    pub confirm: bool,
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default)]
    pub sop_version_no: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct LinkLesson {
    pub lesson_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct SaveAsLesson {
    pub unit_id: i64,
    #[serde(default)]
    pub title: String,
}

pub async fn sync_preview(db: &PgPool, teacher_id: i64, id: i64) -> AppResult<Preview> {
    let ws = store::owned(db, id, teacher_id).await?;
    let content = store::latest(db, id).await?.content;
    let Some((lesson_id, sop_version, changes)) = lesson_sop_diff(db, &ws, &content).await? else {
        return Err(AppError::Conflict("這份學習單沒有連結課例。".to_string()));
    };
    Ok(Preview {
        lesson_id,
        sop_version_no: sop_version.map(|v| v.version_no),
        changes,
        core_suggestions: sync::core_suggestions(&content),
    })
}

pub async fn sync_apply(db: &PgPool, teacher_id: i64, id: i64, req: ApplySync) -> AppResult<Value> {
    let ws = store::owned(db, id, teacher_id).await?;
    if !req.confirm {
        return Err(AppError::bad_request(
            "要同步回課例，需要教師確認（confirm: true）。",
        ));
    }
    let Some(lesson_id) = ws.lesson_id else {
        return Err(AppError::Conflict("這份學習單沒有連結課例。".to_string()));
    };
    owned_lesson(db, lesson_id, teacher_id).await?;
    let current = sop_store::latest(db, lesson_id)
        .await?
        .map(|v| v.version_no);
    if current != req.sop_version_no {
        return Err(AppError::Conflict(
            "課例在你預覽之後改過了，請重新預覽要同步的內容。".to_string(),
        ));
    }
    if req.keys.is_empty() {
        return Err(AppError::bad_request("沒有勾選要同步的項目。"));
    }
    let (sop_version_no, applied) = write_back(db, &ws, lesson_id, &req.keys).await?;
    log_usage(
        db,
        teacher_id,
        "worksheet_sop_synced",
        json!({ "worksheet_id": id, "lesson_id": lesson_id, "sop_version_no": sop_version_no, "applied": applied }),
    )
    .await?;
    Ok(json!({ "sop_version_no": sop_version_no, "applied": applied }))
}

pub async fn link(db: &PgPool, teacher_id: i64, id: i64, req: LinkLesson) -> AppResult<()> {
    store::owned(db, id, teacher_id).await?;
    let sop_version_id = match req.lesson_id {
        Some(lesson_id) => {
            owned_lesson(db, lesson_id, teacher_id).await?;
            sop_store::latest(db, lesson_id).await?.map(|v| v.id)
        }
        None => None,
    };
    store::set_lesson(db, id, req.lesson_id, sop_version_id).await?;
    log_usage(
        db,
        teacher_id,
        "worksheet_linked",
        json!({ "worksheet_id": id, "lesson_id": req.lesson_id }),
    )
    .await?;
    Ok(())
}

pub async fn save_as_lesson(
    db: &PgPool,
    teacher_id: i64,
    id: i64,
    req: SaveAsLesson,
) -> AppResult<()> {
    let ws = store::owned(db, id, teacher_id).await?;
    owned_unit(db, req.unit_id, teacher_id).await?;
    let title = if req.title.trim().is_empty() {
        ws.title.clone()
    } else {
        req.title.trim().to_string()
    };
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(AppError::bad_request(format!(
            "課例名稱最多 {MAX_TITLE_CHARS} 字。"
        )));
    }
    let lesson_id: i64 = sqlx::query_scalar(
        "INSERT INTO lessons (unit_id, title, class_description, my_example)
         VALUES ($1, $2, '', '') RETURNING id",
    )
    .bind(req.unit_id)
    .bind(&title)
    .fetch_one(db)
    .await?;
    if !ws.material.is_empty() {
        sop_store::save_material(
            db,
            teacher_id,
            lesson_id,
            &ws.material.title,
            &ws.material.source.note,
            &ws.material.text(),
        )
        .await?;
    }
    store::set_lesson(db, id, Some(lesson_id), None).await?;
    // Everything the worksheet has becomes the new lesson's SOP: the teacher asked for it.
    let ws = store::owned(db, id, teacher_id).await?;
    let content = store::latest(db, id).await?.content;
    let keys: Vec<String> = sync::diff(&content, &ws.material, &sop::SopContent::default())
        .into_iter()
        .filter(|c| c.action != "remove")
        .map(|c| c.key)
        .collect();
    if !keys.is_empty() {
        write_back(db, &ws, lesson_id, &keys).await?;
    }
    let sop_version_id = sop_store::latest(db, lesson_id).await?.map(|v| v.id);
    store::set_lesson(db, id, Some(lesson_id), sop_version_id).await?;
    log_usage(
        db,
        teacher_id,
        "worksheet_linked",
        json!({ "worksheet_id": id, "lesson_id": lesson_id, "new_lesson": true }),
    )
    .await?;
    Ok(())
}
