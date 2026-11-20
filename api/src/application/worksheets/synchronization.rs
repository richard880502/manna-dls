//! Teacher-confirmed synchronization between a worksheet and its lesson SOP.
//! An application operation owns one transaction: ownership, preview versions, both
//! documents and the research event are checked/written on that same connection.
use crate::audit::log_usage;
use crate::error::{AppError, AppResult};
use crate::sop::{self, store as sop_store};
use crate::teaching_context::{owned_lesson, owned_unit};
use crate::worksheet::store::{self, Worksheet};
use crate::worksheet::sync::{self, Preview};
use crate::worksheet::{Content, MAX_TITLE_CHARS};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{PgConnection, PgPool, Postgres, Transaction};

pub(crate) async fn lesson_sop_diff(
    db: &PgPool,
    ws: &Worksheet,
    content: &Content,
) -> AppResult<Option<(i64, Option<sop_store::Version>, Vec<sync::Change>)>> {
    let mut connection = db.acquire().await?;
    lesson_sop_diff_in(&mut connection, ws, content).await
}

async fn lesson_sop_diff_in(
    db: &mut PgConnection,
    ws: &Worksheet,
    content: &Content,
) -> AppResult<Option<(i64, Option<sop_store::Version>, Vec<sync::Change>)>> {
    let Some(lesson_id) = ws.lesson_id else {
        return Ok(None);
    };
    // A lesson deleted since the link was made: the worksheet stays, unlinked in effect.
    match owned_lesson(&mut *db, lesson_id, ws.teacher_id).await {
        Ok(_) => {}
        Err(AppError::NotFound(_)) => return Ok(None),
        Err(error) => return Err(error),
    }
    let sop_version = sop_store::latest(&mut *db, lesson_id).await?;
    let sop_content = sop_version
        .as_ref()
        .map(|v| v.content.clone())
        .unwrap_or_default();
    let changes = sync::diff(content, &ws.material, &sop_content);
    Ok(Some((lesson_id, sop_version, changes)))
}

/// All compound worksheet/lesson commands take locks in worksheet -> lesson order.
/// Re-read ownership and the link only after the worksheet lock has been acquired.
async fn lock_worksheet(
    tx: &mut Transaction<'_, Postgres>,
    teacher_id: i64,
    id: i64,
) -> AppResult<Worksheet> {
    sqlx::query(
        "SELECT id FROM worksheets WHERE id=$1 AND teacher_id=$2 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(teacher_id)
    .execute(&mut **tx)
    .await?;
    store::owned(&mut **tx, id, teacher_id).await
}

async fn write_back(
    tx: &mut Transaction<'_, Postgres>,
    ws: &Worksheet,
    lesson_id: i64,
    keys: &[String],
) -> AppResult<(i32, usize)> {
    let version = sop_store::latest(&mut **tx, lesson_id).await?;
    let sop_material =
        sop_store::material(&mut **tx, version.as_ref().and_then(|v| v.material_id)).await?;
    let mut sop_content = version.map(|v| v.content).unwrap_or_default();
    let mut content = store::latest(&mut **tx, ws.id).await?.content;
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
    let items = crate::curriculum::by_codes_in(tx, &sop::linked_codes(&sop_content)).await?;
    sop::resolve_links(&mut sop_content, &items).map_err(|code| {
        AppError::bad_request(format!("課綱條目「{code}」不在知識庫裡，不能連結。"))
    })?;
    content.normalise().map_err(AppError::bad_request)?;
    let saved = sop_store::save_content_in_tx(tx, lesson_id, &sop_content).await?;
    store::save_content_in_tx(tx, ws.id, &content, "edit", None, None).await?;
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
    /// New clients also guard the worksheet and its link against a stale preview.
    #[serde(default)]
    pub worksheet_version_no: Option<i32>,
    #[serde(default)]
    pub worksheet_updated_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub lesson_id: Option<i64>,
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
    let mut tx = db.begin().await?;
    // The two documents belong to a single read snapshot without blocking their editors.
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let ws = store::owned(&mut *tx, id, teacher_id).await?;
    let version = store::latest(&mut *tx, id).await?;
    let Some((lesson_id, sop_version, changes)) =
        lesson_sop_diff_in(&mut tx, &ws, &version.content).await?
    else {
        return Err(AppError::Conflict("這份學習單沒有連結課例。".to_string()));
    };
    tx.commit().await?;
    Ok(Preview {
        lesson_id,
        sop_version_no: sop_version.map(|v| v.version_no),
        worksheet_version_no: version.version_no,
        worksheet_updated_at: ws.updated_at,
        changes,
        core_suggestions: sync::core_suggestions(&version.content),
    })
}

pub async fn sync_apply(db: &PgPool, teacher_id: i64, id: i64, req: ApplySync) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let ws = lock_worksheet(&mut tx, teacher_id, id).await?;
    if !req.confirm {
        return Err(AppError::bad_request(
            "要同步回課例，需要教師確認（confirm: true）。",
        ));
    }
    let Some(lesson_id) = ws.lesson_id else {
        return Err(AppError::Conflict("這份學習單沒有連結課例。".to_string()));
    };
    if req.lesson_id.is_some_and(|expected| expected != lesson_id) {
        return Err(AppError::Conflict(
            "學習單連結的課例改過了，請重新預覽要同步的內容。".into(),
        ));
    }
    sop_store::lock_lesson(&mut tx, lesson_id).await?;
    owned_lesson(&mut *tx, lesson_id, teacher_id).await?;
    let current = sop_store::latest(&mut *tx, lesson_id)
        .await?
        .map(|v| v.version_no);
    if current != req.sop_version_no {
        return Err(AppError::Conflict(
            "課例在你預覽之後改過了，請重新預覽要同步的內容。".into(),
        ));
    }
    // Material, settings and links can change without creating a content version.
    if req
        .worksheet_updated_at
        .is_some_and(|expected| expected != ws.updated_at)
    {
        return Err(AppError::Conflict(
            "學習單在你預覽之後改過了，請重新預覽要同步的內容。".into(),
        ));
    }
    let worksheet_version = store::latest(&mut *tx, id).await?;
    if req
        .worksheet_version_no
        .is_some_and(|expected| expected != worksheet_version.version_no)
    {
        return Err(AppError::Conflict(
            "學習單在你預覽之後改過了，請重新預覽要同步的內容。".into(),
        ));
    }
    if req.keys.is_empty() {
        return Err(AppError::bad_request("沒有勾選要同步的項目。"));
    }
    let (sop_version_no, applied) = write_back(&mut tx, &ws, lesson_id, &req.keys).await?;
    log_usage(&mut *tx, teacher_id, "worksheet_sop_synced",
        json!({ "worksheet_id": id, "lesson_id": lesson_id, "sop_version_no": sop_version_no, "applied": applied })).await?;
    tx.commit().await?;
    Ok(json!({ "sop_version_no": sop_version_no, "applied": applied }))
}

pub async fn link(db: &PgPool, teacher_id: i64, id: i64, req: LinkLesson) -> AppResult<()> {
    let mut tx = db.begin().await?;
    lock_worksheet(&mut tx, teacher_id, id).await?;
    let sop_version_id = match req.lesson_id {
        Some(lesson_id) => {
            sop_store::lock_lesson(&mut tx, lesson_id).await?;
            owned_lesson(&mut *tx, lesson_id, teacher_id).await?;
            sop_store::latest(&mut *tx, lesson_id).await?.map(|v| v.id)
        }
        None => None,
    };
    store::set_lesson(&mut *tx, id, req.lesson_id, sop_version_id).await?;
    log_usage(
        &mut *tx,
        teacher_id,
        "worksheet_linked",
        json!({ "worksheet_id": id, "lesson_id": req.lesson_id }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn save_as_lesson(
    db: &PgPool,
    teacher_id: i64,
    id: i64,
    req: SaveAsLesson,
) -> AppResult<()> {
    let mut tx = db.begin().await?;
    let ws = lock_worksheet(&mut tx, teacher_id, id).await?;
    // Hold the destination unit against deletion until the new lesson is complete.
    sqlx::query("SELECT id FROM units WHERE id=$1 FOR SHARE")
        .bind(req.unit_id)
        .execute(&mut *tx)
        .await?;
    owned_unit(&mut *tx, req.unit_id, teacher_id).await?;
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
    .fetch_one(&mut *tx)
    .await?;
    if !ws.material.is_empty() {
        sop_store::save_material_in_tx(
            &mut tx,
            teacher_id,
            lesson_id,
            &ws.material.title,
            &ws.material.source.note,
            &ws.material.text(),
        )
        .await?;
    }
    let content = store::latest(&mut *tx, id).await?.content;
    let keys: Vec<String> = sync::diff(&content, &ws.material, &sop::SopContent::default())
        .into_iter()
        .filter(|c| c.action != "remove")
        .map(|c| c.key)
        .collect();
    if !keys.is_empty() {
        write_back(&mut tx, &ws, lesson_id, &keys).await?;
    }
    let sop_version_id = sop_store::latest(&mut *tx, lesson_id).await?.map(|v| v.id);
    store::set_lesson(&mut *tx, id, Some(lesson_id), sop_version_id).await?;
    log_usage(
        &mut *tx,
        teacher_id,
        "worksheet_linked",
        json!({ "worksheet_id": id, "lesson_id": lesson_id, "new_lesson": true }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
