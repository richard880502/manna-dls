//! The job of one picture row: wait for a slot (at most `IMAGE_CONCURRENCY` pictures at a time
//! across the server), make the picture (retried once after a rate limit, a 5xx or a dropped
//! connection), re-encode it as JPEG and store it.
//!
//! A run is identified by `started_at`: every write is guarded by it (and by the row still being
//! open), so a run that was taken over (restart) or replaced by the teacher (重畫, upload,
//! 拿掉) stops quietly at its next write. A slot that is no longer in the worksheet when the job
//! starts is skipped without calling the model.
//!
//! Every model call is an `ai_interactions` row (module `worksheet_image`) with the length and
//! SHA-256 of the prompt (and of the teacher's note), the request settings, and on success the
//! picture's size, dimensions, SHA-256 and token counts: never a prompt or a picture.

use std::time::Instant;

use chrono::{DateTime, Utc};
use serde_json::json;

use super::client::{ImageError, ImageReply, MODERATION, OUTPUT_COMPRESSION, OUTPUT_FORMAT};
use super::{
    failure_of, find_slot, normalize_blocking, picture_prompt, safety_identifier, storage_key,
    style, MIME, MODULE, MSG_JOB_GONE, MSG_JOB_INTERNAL, MSG_JOB_NO_STORAGE, MSG_JOB_STORE_FAILED,
    SCAFFOLD,
};
use crate::ai::pipeline::{finish_interaction, insert_interaction, Finish, InteractionMeta};
use crate::audit::{log_usage, scaffold_id};
use crate::auth::sha256_hex;
use crate::error::AppResult;
use crate::AppState;

/// Starts the job of the row opened at `run`. The caller has read its response already.
pub fn spawn(state: &AppState, image_id: i64, run: DateTime<Utc>) {
    let state = state.clone();
    // Registered now, so `expire_stale` never takes a queued row for a dead one.
    let live = state.images.track(image_id);
    tokio::spawn(async move {
        let _live = live;
        let work = {
            let state = state.clone();
            async move { run_job(&state, image_id, run).await }
        };
        let failed = crate::jobs::run("worksheet_image", image_id, work)
            .await
            .is_err();
        if failed {
            if let Err(e) = close(
                &state,
                image_id,
                run,
                "failed",
                "internal",
                MSG_JOB_INTERNAL,
            )
            .await
            {
                tracing::error!(image_id, error = %e, "could not close the picture row");
            }
        }
    });
}

/// The run's guard: this row, this run, still open.
const RUN_GUARD: &str = "id = $1 AND started_at = $2 AND status IN ('queued', 'running')";

async fn close(
    state: &AppState,
    id: i64,
    run: DateTime<Utc>,
    status: &str,
    kind: &str,
    message: &str,
) -> sqlx::Result<bool> {
    let done = sqlx::query(&format!(
        "UPDATE worksheet_images
            SET status = $3, error_kind = $4, message = $5, finished_at = now()
          WHERE {RUN_GUARD}"
    ))
    .bind(id)
    .bind(run)
    .bind(status)
    .bind(kind)
    .bind(message)
    .execute(&state.db)
    .await?;
    Ok(done.rows_affected() > 0)
}

#[derive(sqlx::FromRow)]
struct JobRow {
    worksheet_id: i64,
    teacher_id: i64,
    slot_id: String,
    hint: String,
    regeneration: bool,
}

async fn run_job(state: &AppState, id: i64, run: DateTime<Utc>) -> AppResult<()> {
    let db = &state.db;
    // At most `IMAGE_CONCURRENCY` pictures at once. A row waiting here is `queued`.
    let _slot = state
        .images
        .slots
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| anyhow::anyhow!("picture slots closed: {e}"))?;
    let started: Option<JobRow> = sqlx::query_as(&format!(
        "UPDATE worksheet_images SET status = 'running' WHERE {RUN_GUARD}
         RETURNING worksheet_id, teacher_id, slot_id, hint, regeneration"
    ))
    .bind(id)
    .bind(run)
    .fetch_optional(db)
    .await?;
    let Some(row) = started else {
        return Ok(());
    };
    if !state.images.enabled() {
        close(state, id, run, "failed", "disabled", super::MSG_DISABLED).await?;
        return Ok(());
    }
    let Some(storage) = state.storage.clone() else {
        close(state, id, run, "failed", "storage", MSG_JOB_NO_STORAGE).await?;
        return Ok(());
    };
    // The slot as it is now: a block regenerated meanwhile has new slots, and this one is not
    // worth paying for any more.
    let latest = super::super::store::latest(db, row.worksheet_id).await?;
    let Some((_, slot)) = find_slot(&latest.content, &row.slot_id) else {
        close(state, id, run, "skipped", "gone", MSG_JOB_GONE).await?;
        return Ok(());
    };
    let slot = slot.clone();
    let lesson_id: Option<i64> =
        sqlx::query_scalar("SELECT lesson_id FROM worksheets WHERE id = $1")
            .bind(row.worksheet_id)
            .fetch_optional(db)
            .await?
            .flatten();
    let style = style(db).await?;
    let prompt = picture_prompt(&style, &slot, &row.hint);
    let config = state.images.config();
    sqlx::query(&format!(
        "UPDATE worksheet_images
            SET model = $3, size = $4, quality = $5, style_version_no = $6,
                prompt_chars = $7, prompt_sha256 = $8
          WHERE {RUN_GUARD}"
    ))
    .bind(id)
    .bind(run)
    .bind(state.images.model())
    .bind(&config.size)
    .bind(&config.quality)
    .bind(style.version_no)
    .bind(prompt.chars().count() as i32)
    .bind(sha256_hex(&prompt))
    .execute(db)
    .await?;

    let meta = InteractionMeta {
        teacher_id: row.teacher_id,
        module: MODULE,
        unit_id: None,
        lesson_id,
        scaffold_id: scaffold_id(db, SCAFFOLD).await?,
        prompt_version_id: style.id,
    };
    let hint = row.hint.trim();
    let snapshot = json!({
        "worksheet_id": row.worksheet_id,
        "image_id": id,
        "slot_id": row.slot_id,
        "kind": slot.kind,
        "regeneration": row.regeneration,
        "prompt": { "chars": prompt.chars().count(), "sha256": sha256_hex(&prompt),
                    "style_version_no": style.version_no },
        "teacher_note": if hint.is_empty() { json!(null) }
                        else { json!({ "chars": hint.chars().count(), "sha256": sha256_hex(hint) }) },
        "request": { "model": state.images.model(), "size": config.size, "quality": config.quality,
                     "n": 1, "output_format": OUTPUT_FORMAT,
                     "output_compression": OUTPUT_COMPRESSION, "moderation": MODERATION },
    });
    let safety = safety_identifier(row.teacher_id);
    let started_at = Instant::now();
    let reply = call(state, &meta, &snapshot, &prompt, &safety).await?;
    let latency_ms = started_at.elapsed().as_millis().min(i32::MAX as u128) as i64;
    let (reply, interaction_id) = match reply {
        (Ok(r), iid) => (r, iid),
        (Err(e), iid) => {
            let (kind, message) = failure_of(&e);
            sqlx::query(&format!(
                "UPDATE worksheet_images SET interaction_id = $3 WHERE {RUN_GUARD}"
            ))
            .bind(id)
            .bind(run)
            .bind(iid)
            .execute(db)
            .await?;
            if close(state, id, run, "failed", kind, &message).await? {
                tracing::info!(
                    image_id = id,
                    worksheet_id = row.worksheet_id,
                    kind,
                    latency_ms,
                    "worksheet picture failed"
                );
                finished_log(state, &row, id, "failed", Some(kind), 0, latency_ms).await?;
            }
            return Ok(());
        }
    };

    let picture = match normalize_blocking(reply.bytes).await? {
        Ok(p) => p,
        Err(_) => {
            close(
                state,
                id,
                run,
                "failed",
                "invalid",
                &ImageError::Invalid(String::new()).slot_message(),
            )
            .await?;
            return Ok(());
        }
    };
    let key = storage_key(row.worksheet_id, id);
    if let Err(e) = storage.put(&key, MIME, picture.bytes.clone().into()).await {
        tracing::error!(image_id = id, error = %e, "storing a worksheet picture failed");
        close(state, id, run, "failed", "storage", MSG_JOB_STORE_FAILED).await?;
        return Ok(());
    }
    let stored = sqlx::query(&format!(
        "UPDATE worksheet_images
            SET status = 'done', storage_key = $3, content_type = $4, bytes = $5, width = $6,
                height = $7, sha256 = $8, interaction_id = $9, message = NULL, finished_at = now()
          WHERE {RUN_GUARD}"
    ))
    .bind(id)
    .bind(run)
    .bind(&key)
    .bind(MIME)
    .bind(picture.bytes.len() as i32)
    .bind(picture.width as i32)
    .bind(picture.height as i32)
    .bind(sha256_hex(&picture.bytes))
    .bind(interaction_id)
    .execute(db)
    .await?;
    if stored.rows_affected() == 0 {
        // Replaced or taken over meanwhile: the picture is not this row's any more.
        let _ = storage.delete(&key).await;
        return Ok(());
    }
    tracing::info!(
        image_id = id,
        worksheet_id = row.worksheet_id,
        bytes = picture.bytes.len(),
        width = picture.width,
        height = picture.height,
        latency_ms,
        "worksheet picture stored"
    );
    finished_log(
        state,
        &row,
        id,
        "done",
        None,
        picture.bytes.len(),
        latency_ms,
    )
    .await?;
    Ok(())
}

async fn finished_log(
    state: &AppState,
    row: &JobRow,
    id: i64,
    status: &str,
    kind: Option<&str>,
    bytes: usize,
    latency_ms: i64,
) -> sqlx::Result<()> {
    log_usage(
        &state.db,
        row.teacher_id,
        "worksheet_image_finished",
        json!({
            "worksheet_id": row.worksheet_id, "image_id": id, "slot_id": row.slot_id,
            "status": status, "error_kind": kind, "regeneration": row.regeneration,
            "bytes": bytes, "latency_ms": latency_ms,
        }),
    )
    .await
}

/// The model call, logged; once more after a rate limit, a 5xx or a dropped connection.
/// Returns the reply and the interaction of the last attempt.
async fn call(
    state: &AppState,
    meta: &InteractionMeta,
    snapshot: &serde_json::Value,
    prompt: &str,
    safety: &str,
) -> AppResult<(Result<ImageReply, ImageError>, i64)> {
    let db = &state.db;
    let mut retry_of = None;
    let mut attempt = 1;
    loop {
        let interaction = insert_interaction(
            db,
            meta,
            state.images.provider_name(),
            state.images.model(),
            snapshot,
            attempt,
            retry_of,
        )
        .await?;
        let started = Instant::now();
        let reply = state.images.generate(prompt, safety).await;
        let latency_ms = started.elapsed().as_millis().min(i32::MAX as u128) as i32;
        let finish = match &reply {
            Ok(r) => Finish {
                status: "shown",
                output: Some(json!({
                    "bytes": r.bytes.len(),
                    "sha256": sha256_hex(&r.bytes),
                    "usage": r.usage,
                })),
                citations: Vec::new(),
                check_results: None,
                model_version: None,
                error_detail: None,
                latency_ms,
            },
            Err(e) => Finish {
                status: e.interaction_status(),
                output: None,
                citations: Vec::new(),
                check_results: None,
                model_version: None,
                error_detail: Some(e.to_string()),
                latency_ms,
            },
        };
        finish_interaction(db, interaction, finish).await?;
        match reply {
            Err(e) if attempt == 1 && e.retryable() => {
                retry_of = Some(interaction);
                attempt = 2;
                // A moment before the second try (a rate limit needs it); the fake does not.
                let fake = state.images.config().provider == super::ImageProviderKind::Fake;
                tokio::time::sleep(std::time::Duration::from_millis(if fake {
                    10
                } else {
                    2000
                }))
                .await;
            }
            other => return Ok((other, interaction)),
        }
    }
}
