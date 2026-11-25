use super::super::comments::STATUS_OPEN;
use super::super::images::actions::{self, Added, PictureAction};
use super::super::{store, Block, Content};
use super::rules::{changes, Changes};
use super::{
    MSG_ANOTHER_RUNNING, MSG_CHANGED_MEANWHILE, MSG_EARLIER_CHANGED, MSG_STALE_RUN, STATUS_FAILED,
    STATUS_PROPOSED, STATUS_REJECTED, STATUS_RUNNING,
};
use crate::error::{AppError, AppResult};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{types::Json, PgPool};

// ---------------------------------------------------------------- proposals

#[derive(Debug, Clone, Serialize)]
pub struct Proposal {
    pub id: i64,
    pub worksheet_id: i64,
    pub block_id: String,
    /// `running` | `proposed` | `accepted` | `rejected` | `stale` | `failed`
    pub status: String,
    pub instruction: String,
    pub comment_ids: Vec<i64>,
    pub whole_block: bool,
    /// The how-many-th proposal for this block (「再改一次」 asks again with the next one).
    pub round: i32,
    /// What Manna says it changed (or its question, when it changed nothing).
    pub reply: String,
    /// Why there is no proposal (`failed`, `stale`).
    pub error: Option<String>,
    pub base_version_no: i32,
    /// The block as it was sent to Manna.
    pub before: Block,
    /// The revised block (`proposed` and later).
    pub block: Option<Block>,
    pub changes: Option<Changes>,
    /// What Manna proposes to do with the block's pictures; nothing is drawn or removed until
    /// the teacher accepts (docs/API.md「改圖片」).
    pub picture_actions: Vec<PictureAction>,
    /// The pictures the teacher attached to the request.
    pub images: Vec<crate::prep::images::PrepImage>,
    pub interaction_id: Option<i64>,
    pub accepted_version_no: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub decided_at: Option<DateTime<Utc>>,
    /// When the teacher took the accepted proposal back (復原); it stays `accepted`.
    pub undone_at: Option<DateTime<Utc>>,
    /// When a proposal that was not adopted at the time was adopted later from the 修改歷程, and the
    /// version that wrote (it stays as it was decided then).
    pub adopted_later_at: Option<DateTime<Utc>>,
    pub adopted_later_version_no: Option<i32>,
    /// The earlier attempts of the same round (oldest first) that 再改一次 set aside: made from
    /// the same block, not adopted or given up yet. Any of them can still be adopted.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub earlier: Vec<Proposal>,
    /// While the revision waits in the AI queue: where it stands (docs/API.md「AI 工作佇列」).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue: Option<crate::progress::QueueInfo>,
}

#[derive(sqlx::FromRow)]
struct Row {
    picture_actions: Json<Vec<PictureAction>>,
    id: i64,
    worksheet_id: i64,
    block_id: String,
    base_version_no: i32,
    base_block: Value,
    instruction: String,
    comment_ids: Vec<i64>,
    whole_block: bool,
    round: i32,
    status: String,
    proposed: Option<Value>,
    reply: String,
    error: Option<String>,
    interaction_id: Option<i64>,
    accepted_version_no: Option<i32>,
    created_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    decided_at: Option<DateTime<Utc>>,
    undone_at: Option<DateTime<Utc>>,
    adopted_later_at: Option<DateTime<Utc>>,
    adopted_later_version_no: Option<i32>,
    set_aside_at: Option<DateTime<Utc>>,
    round_closed_at: Option<DateTime<Utc>>,
}

const COLS: &str = "id, worksheet_id, block_id, base_version_no, base_block, instruction,
    comment_ids, whole_block, round, status, proposed, reply, error, interaction_id,
    accepted_version_no, created_at, finished_at, decided_at, picture_actions, undone_at,
    set_aside_at, round_closed_at, adopted_later_at, adopted_later_version_no";

impl From<Row> for Proposal {
    fn from(r: Row) -> Self {
        let before: Block = serde_json::from_value(r.base_block).unwrap_or_default();
        let block: Option<Block> = r.proposed.and_then(|v| serde_json::from_value(v).ok());
        Self {
            changes: block.as_ref().map(|b| changes(&before, b)),
            picture_actions: r.picture_actions.0,
            images: Vec::new(),
            id: r.id,
            worksheet_id: r.worksheet_id,
            block_id: r.block_id,
            status: r.status,
            instruction: r.instruction,
            comment_ids: r.comment_ids,
            whole_block: r.whole_block,
            round: r.round,
            reply: r.reply,
            error: r.error,
            base_version_no: r.base_version_no,
            before,
            block,
            interaction_id: r.interaction_id,
            accepted_version_no: r.accepted_version_no,
            created_at: r.created_at,
            finished_at: r.finished_at,
            decided_at: r.decided_at,
            undone_at: r.undone_at,
            adopted_later_at: r.adopted_later_at,
            adopted_later_version_no: r.adopted_later_version_no,
            earlier: Vec::new(),
            queue: None,
        }
    }
}

/// A revision still `running` long after its calls must have timed out died with the server.
/// One whose job waits in the AI queue (or runs from it) is not stale, while `queued` says the
/// queue is on: the queue's leases tell whether its server is alive.
pub async fn expire_stale(
    db: &PgPool,
    worksheet_id: i64,
    after_secs: u64,
    queued: bool,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE worksheet_block_proposals p
            SET status = 'failed', error = $2, finished_at = now()
          WHERE p.worksheet_id = $1 AND p.status = 'running'
            AND p.created_at < now() - make_interval(secs => $3)
            AND NOT ($4 AND EXISTS (SELECT 1 FROM ai_jobs j
                             WHERE j.ref_kind = 'worksheet_proposal' AND j.ref_id = p.id
                               AND j.status IN ('queued', 'running')))",
    )
    .bind(worksheet_id)
    .bind(MSG_STALE_RUN)
    .bind(after_secs as f64)
    .bind(queued)
    .execute(db)
    .await?;
    Ok(())
}

/// The latest proposal of every block that still waits for the teacher: running, proposed, or
/// failed / stale and not closed yet.
pub async fn open(db: &PgPool, worksheet_id: i64) -> sqlx::Result<Vec<Proposal>> {
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {COLS} FROM (
             SELECT DISTINCT ON (block_id) * FROM worksheet_block_proposals
              WHERE worksheet_id = $1 ORDER BY block_id, id DESC) latest
          WHERE decided_at IS NULL AND status IN ('running', 'proposed', 'failed', 'stale')
          ORDER BY id"
    ))
    .bind(worksheet_id)
    .fetch_all(db)
    .await?;
    let mut proposals: Vec<Proposal> = rows.into_iter().map(Proposal::from).collect();
    let ids: Vec<i64> = proposals.iter().map(|p| p.id).collect();
    let mut images = crate::prep::images::for_proposals(db, &ids).await?;
    for p in &mut proposals {
        p.images = images.remove(&p.id).unwrap_or_default();
    }
    // The attempts 再改一次 set aside stay adoptable until the round ends.
    let earlier: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {COLS} FROM worksheet_block_proposals
          WHERE worksheet_id = $1 AND set_aside_at IS NOT NULL AND round_closed_at IS NULL
            AND status = 'rejected' AND proposed IS NOT NULL ORDER BY id"
    ))
    .bind(worksheet_id)
    .fetch_all(db)
    .await?;
    let earlier: Vec<Proposal> = earlier.into_iter().map(Proposal::from).collect();
    for p in &mut proposals {
        p.earlier = earlier
            .iter()
            .filter(|e| e.block_id == p.block_id && e.id < p.id && e.before == p.before)
            .cloned()
            .collect();
    }
    Ok(proposals)
}

/// Every proposal ever made for a block, oldest first (the 修改歷程 lists the ones that were not
/// adopted).
pub async fn for_block(
    db: &PgPool,
    worksheet_id: i64,
    block_id: &str,
) -> sqlx::Result<Vec<Proposal>> {
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {COLS} FROM worksheet_block_proposals
          WHERE worksheet_id = $1 AND block_id = $2 ORDER BY id"
    ))
    .bind(worksheet_id)
    .bind(block_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(Proposal::from).collect())
}

pub async fn get(db: &PgPool, worksheet_id: i64, id: i64) -> AppResult<Proposal> {
    let row: Option<Row> = sqlx::query_as(&format!(
        "SELECT {COLS} FROM worksheet_block_proposals WHERE id = $1 AND worksheet_id = $2"
    ))
    .bind(id)
    .bind(worksheet_id)
    .fetch_optional(db)
    .await?;
    let mut proposal = row
        .map(Proposal::from)
        .ok_or_else(|| AppError::not_found("找不到這個修改。"))?;
    proposal.images = crate::prep::images::for_proposals(db, &[id])
        .await?
        .remove(&id)
        .unwrap_or_default();
    Ok(proposal)
}

pub(crate) async fn decision(
    db: impl sqlx::PgExecutor<'_>,
    interaction_id: Option<i64>,
    teacher_id: i64,
    decision: &str,
    note: &str,
) -> sqlx::Result<()> {
    let Some(interaction_id) = interaction_id else {
        return Ok(());
    };
    sqlx::query(
        "INSERT INTO teacher_decisions (ai_interaction_id, teacher_id, item_index, decision, reason_note)
         VALUES ($1, $2, 0, $3, $4)",
    )
    .bind(interaction_id)
    .bind(teacher_id)
    .bind(decision)
    .bind(note)
    .execute(db)
    .await?;
    Ok(())
}

/// What the block's previous proposal said, when the teacher asks again (「再改一次」): its open
/// proposal is set aside (a proposed one counts as discarded), a failed or stale one is closed.
/// Returns the previous reply, if any.
pub async fn set_aside_previous(
    db: &PgPool,
    worksheet_id: i64,
    block_id: &str,
    teacher_id: i64,
) -> sqlx::Result<Option<String>> {
    let previous: Option<(i64, String, String, Option<i64>)> = sqlx::query_as(
        "SELECT id, status, reply, interaction_id FROM worksheet_block_proposals
          WHERE worksheet_id = $1 AND block_id = $2 ORDER BY id DESC LIMIT 1",
    )
    .bind(worksheet_id)
    .bind(block_id)
    .fetch_optional(db)
    .await?;
    let Some((id, status, reply, interaction_id)) = previous else {
        return Ok(None);
    };
    let set_aside = sqlx::query(
        "UPDATE worksheet_block_proposals
            SET status = CASE WHEN status = 'proposed' THEN 'rejected' ELSE status END,
                set_aside_at = CASE WHEN status = 'proposed' THEN now() ELSE set_aside_at END,
                decided_at = now()
          WHERE id = $1 AND decided_at IS NULL AND status IN ('proposed', 'failed', 'stale')",
    )
    .bind(id)
    .execute(db)
    .await?;
    if set_aside.rows_affected() > 0 && status == STATUS_PROPOSED {
        decision(
            db,
            interaction_id,
            teacher_id,
            "reject",
            "學習單：逐區修改，再改一次",
        )
        .await?;
    }
    Ok((!reply.trim().is_empty()).then_some(reply))
}

/// How many proposals the block has had.
/// Whether a revision of the worksheet is running (only one runs at a time).
pub async fn any_running(db: &PgPool, worksheet_id: i64) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM worksheet_block_proposals
                         WHERE worksheet_id = $1 AND status = 'running')",
    )
    .bind(worksheet_id)
    .fetch_one(db)
    .await
}

pub async fn rounds(db: &PgPool, worksheet_id: i64, block_id: &str) -> sqlx::Result<i64> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM worksheet_block_proposals WHERE worksheet_id = $1 AND block_id = $2",
    )
    .bind(worksheet_id)
    .bind(block_id)
    .fetch_one(db)
    .await
}

/// Makes the new proposal a `block_revise` job of the AI queue (in the same transaction).
pub struct ProposalQueue<'a> {
    pub max_queued: usize,
    pub track: &'a str,
}

pub struct NewProposal<'a> {
    pub worksheet_id: i64,
    pub teacher_id: i64,
    pub block: &'a Block,
    pub base_version_no: i32,
    pub instruction: &'a str,
    pub comment_ids: &'a [i64],
    pub whole_block: bool,
    pub round: i32,
    pub queue: Option<ProposalQueue<'a>>,
}

/// Opens a `running` proposal; another one running on the worksheet is a 409. With a queue the
/// proposal waits in the AI queue as `running` (the page shows 「Manna 正在改」 and, with
/// `queue`, where it stands); its job is made in the same transaction.
pub async fn insert_running(db: &PgPool, new: NewProposal<'_>) -> AppResult<Proposal> {
    let mut tx = db.begin().await?;
    let row: Result<Row, sqlx::Error> = sqlx::query_as(&format!(
        "INSERT INTO worksheet_block_proposals
           (worksheet_id, teacher_id, block_id, base_version_no, base_block, instruction,
            comment_ids, whole_block, round, status)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'running')
         RETURNING {COLS}"
    ))
    .bind(new.worksheet_id)
    .bind(new.teacher_id)
    .bind(&new.block.id)
    .bind(new.base_version_no)
    .bind(Json(new.block))
    .bind(new.instruction)
    .bind(new.comment_ids)
    .bind(new.whole_block)
    .bind(new.round)
    .fetch_one(&mut *tx)
    .await;
    match row {
        Ok(r) => {
            if let Some(q) = &new.queue {
                crate::queue::enqueue_tx(
                    &mut tx,
                    crate::queue::NewJob {
                        kind: crate::queue::handlers::KIND_BLOCK_REVISE,
                        teacher_id: new.teacher_id,
                        priority: 0,
                        params: serde_json::json!({ "proposal_id": r.id, "track": q.track }),
                        ref_kind: "worksheet_proposal",
                        ref_id: r.id,
                    },
                    q.max_queued,
                )
                .await?;
            }
            tx.commit().await?;
            Ok(r.into())
        }
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            Err(AppError::Conflict(MSG_ANOTHER_RUNNING.to_string()))
        }
        Err(e) => Err(e.into()),
    }
}

/// Ends a running proposal: with the revised block, or with why there is none. A proposal that
/// is no longer running (expired meanwhile) is left as it is.
pub async fn finish(
    db: &PgPool,
    id: i64,
    outcome: Result<(Block, String, Vec<PictureAction>), String>,
    interaction_id: Option<i64>,
) -> sqlx::Result<()> {
    let (status, proposed, reply, error, pictures) = match outcome {
        Ok((block, reply, pictures)) => (STATUS_PROPOSED, Some(Json(block)), reply, None, pictures),
        Err(message) => (
            STATUS_FAILED,
            None,
            String::new(),
            Some(message),
            Vec::new(),
        ),
    };
    sqlx::query(
        "UPDATE worksheet_block_proposals
            SET status = $2, proposed = $3, reply = $4, error = $5, interaction_id = $6,
                picture_actions = $7, finished_at = now()
          WHERE id = $1 AND status = 'running'",
    )
    .bind(id)
    .bind(status)
    .bind(proposed)
    .bind(reply)
    .bind(error)
    .bind(interaction_id)
    .bind(Json(pictures))
    .execute(db)
    .await?;
    Ok(())
}

pub enum Accepted {
    /// Written as a new version (`cause = "revise"`), unless the block's own content did not
    /// change (`changed = false`: only pictures). `added` and `content` (the version as saved)
    /// are for running the picture actions; `history_id` is the kept earlier state of the block
    /// (an undo takes it back), which also needs the pictures' part once they have run.
    Done {
        proposal: Box<Proposal>,
        added: Added,
        content: Box<Content>,
        changed: bool,
        history_id: Option<i64>,
        version_no: i32,
        /// Which attempt of the round was adopted, out of how many, and their proposal ids.
        attempt: (usize, usize),
        round_ids: Vec<i64>,
    },
    /// The block changed since the proposal was made: nothing was written.
    Stale(Box<Proposal>),
}

/// 採用: in one transaction, checks that the block in the latest version is still exactly the
/// one the proposal was made from, keeps it in the block history, writes the revised block as a
/// new version, marks the notes it was made from `applied` and records the decision.
pub async fn accept(
    db: &PgPool,
    worksheet_id: i64,
    id: i64,
    teacher_id: i64,
) -> AppResult<Accepted> {
    let mut tx = db.begin().await?;
    sqlx::query("SELECT id FROM worksheets WHERE id = $1 FOR UPDATE")
        .bind(worksheet_id)
        .execute(&mut *tx)
        .await?;
    let row: Row = sqlx::query_as(&format!(
        "SELECT {COLS} FROM worksheet_block_proposals WHERE id = $1 AND worksheet_id = $2"
    ))
    .bind(id)
    .bind(worksheet_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::not_found("找不到這個修改。"))?;
    // An attempt that 再改一次 set aside can still be adopted while its round is open.
    let earlier_attempt = row.status == STATUS_REJECTED
        && row.set_aside_at.is_some()
        && row.round_closed_at.is_none()
        && row.proposed.is_some();
    match row.status.as_str() {
        STATUS_PROPOSED => {}
        STATUS_RUNNING => {
            return Err(AppError::Conflict(
                "Manna 還在改這一區，好了之後再採用。".to_string(),
            ))
        }
        _ if earlier_attempt => {
            let running: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM worksheet_block_proposals
                                 WHERE worksheet_id = $1 AND block_id = $2 AND status = 'running')",
            )
            .bind(worksheet_id)
            .bind(&row.block_id)
            .fetch_one(&mut *tx)
            .await?;
            if running {
                return Err(AppError::Conflict(
                    "Manna 還在改這一區，好了之後再採用。".to_string(),
                ));
            }
        }
        _ => return Err(AppError::Conflict("這個修改已經處理過了。".to_string())),
    }
    let interaction_id = row.interaction_id;
    let comment_ids = row.comment_ids.clone();
    let proposal = Proposal::from(row);
    let Some(revised) = proposal.block.clone() else {
        return Err(AppError::Conflict("這個修改已經處理過了。".to_string()));
    };
    let latest: (Value, i32) = sqlx::query_as(
        "SELECT content, version_no FROM worksheet_versions WHERE worksheet_id = $1
          ORDER BY version_no DESC LIMIT 1",
    )
    .bind(worksheet_id)
    .fetch_one(&mut *tx)
    .await?;
    let latest_no = latest.1;
    let mut content: Content = serde_json::from_value(latest.0).unwrap_or_default();
    let pos = content
        .blocks
        .iter()
        .position(|b| b.id == proposal.block_id && *b == proposal.before);
    let Some(pos) = pos else {
        if earlier_attempt {
            // Nothing changes: the other attempts of the round may still fit.
            return Err(AppError::Conflict(MSG_EARLIER_CHANGED.to_string()));
        }
        sqlx::query(
            "UPDATE worksheet_block_proposals SET status = 'stale', error = $2 WHERE id = $1",
        )
        .bind(id)
        .bind(MSG_CHANGED_MEANWHILE)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(Accepted::Stale(Box::new(get(db, worksheet_id, id).await?)));
    };
    let previous = std::mem::replace(&mut content.blocks[pos], revised);
    // The pictures to add become slots of the block, part of the same version.
    let added = actions::add_slots(&mut content, &proposal.picture_actions, &format!("p{id}"));
    content
        .normalise()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("revised block failed validation: {e}")))?;
    // A proposal that only redraws or removes pictures changes no text and adds no slot: its
    // step is written once its pictures have run (`record_picture_step`), if they changed any.
    let changed = content.blocks[pos] != previous;
    let (version_no, history_id) = if changed {
        let saved = store::insert_version_from(
            &mut tx,
            worksheet_id,
            &content,
            "revise",
            Some(serde_json::json!({
                "cause": "block_proposal", "proposal_id": id, "interaction_id": interaction_id,
                "round": proposal.round, "block_ids": [proposal.block_id],
            })),
        )
        .await?;
        let history_id: i64 = sqlx::query_scalar(
            "INSERT INTO worksheet_block_history
               (worksheet_id, block_id, block, interaction_id, proposal_id, version_no, after_block)
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        )
        .bind(worksheet_id)
        .bind(&previous.id)
        .bind(Json(&previous))
        .bind(interaction_id)
        .bind(id)
        .bind(saved.version_no)
        .bind(Json(&content.blocks[pos]))
        .fetch_one(&mut *tx)
        .await?;
        (saved.version_no, Some(history_id))
    } else {
        (latest_no, None)
    };
    // The round: this attempt and the others made from the same block. Adopting one ends it.
    let candidates: Vec<(i64, Value, String)> = sqlx::query_as(
        "SELECT id, base_block, status FROM worksheet_block_proposals
          WHERE worksheet_id = $1 AND block_id = $2 AND round_closed_at IS NULL
            AND proposed IS NOT NULL
            AND ((status = 'rejected' AND set_aside_at IS NOT NULL) OR status = 'proposed')
          ORDER BY id",
    )
    .bind(worksheet_id)
    .bind(&proposal.block_id)
    .fetch_all(&mut *tx)
    .await?;
    let round_ids: Vec<i64> = candidates
        .iter()
        .filter(|(_, base, _)| {
            serde_json::from_value::<Block>(base.clone()).is_ok_and(|b| b == proposal.before)
        })
        .map(|(i, _, _)| *i)
        .collect();
    let attempt_no = round_ids.iter().position(|i| *i == id).map_or(1, |k| k + 1);
    let round_ids = if round_ids.contains(&id) {
        round_ids
    } else {
        vec![id]
    };
    // A newer attempt still proposed is turned down for this one (its own reject is recorded;
    // the earlier rejections stay as they were).
    let newer: Vec<(i64, Option<i64>)> = sqlx::query_as(
        "SELECT id, interaction_id FROM worksheet_block_proposals
          WHERE worksheet_id = $1 AND block_id = $2 AND status = 'proposed' AND id <> $3",
    )
    .bind(worksheet_id)
    .bind(&proposal.block_id)
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    for (other, other_interaction) in newer {
        sqlx::query(
            "UPDATE worksheet_block_proposals
                SET status = 'rejected', decided_at = now(), round_closed_at = now()
              WHERE id = $1",
        )
        .bind(other)
        .execute(&mut *tx)
        .await?;
        decision(
            &mut *tx,
            other_interaction,
            teacher_id,
            "reject",
            "學習單：採用了同一輪的另一次建議",
        )
        .await?;
    }
    sqlx::query(
        "UPDATE worksheet_block_proposals SET round_closed_at = now()
          WHERE id = ANY($1) AND id <> $2 AND round_closed_at IS NULL",
    )
    .bind(&round_ids)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE worksheet_block_proposals
            SET status = 'accepted', accepted_version_no = $2, decided_at = now(),
                round_closed_at = now()
          WHERE id = $1",
    )
    .bind(id)
    .bind(version_no)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE worksheet_comments SET status = 'applied', proposal_id = $1, updated_at = now()
          WHERE worksheet_id = $2 AND id = ANY($3) AND status = $4",
    )
    .bind(id)
    .bind(worksheet_id)
    .bind(&comment_ids)
    .bind(STATUS_OPEN)
    .execute(&mut *tx)
    .await?;
    decision(
        &mut *tx,
        interaction_id,
        teacher_id,
        "adopt",
        "學習單：採用逐區修改",
    )
    .await?;
    tx.commit().await?;
    Ok(Accepted::Done {
        proposal: Box::new(get(db, worksheet_id, id).await?),
        added,
        content: Box::new(content),
        changed,
        history_id,
        version_no,
        attempt: (attempt_no, round_ids.len()),
        round_ids,
    })
}

/// What the pictures of an accepted proposal did, once they have run: kept with the step so an
/// undo can give the pictures back too, and so an earlier step can be restored with its pictures.
///
/// * a step that changed the block already has its version and kept state: the pictures' part is
///   added to them;
/// * a proposal that changed only pictures gets its step now (a version with the same content,
///   `cause = "revise"`, and a kept state), if any picture really changed.
///
/// Returns the version the proposal ended up in.
pub async fn record_picture_step(
    db: &PgPool,
    proposal: &Proposal,
    done: &PicturesDone,
) -> AppResult<i32> {
    let ws_id = proposal.worksheet_id;
    let snapshot = crate::worksheet::images::undo::snapshot_json(&done.after);
    if done.changed {
        if let Some(history_id) = done.history_id {
            if !done.undo.is_empty() {
                sqlx::query("UPDATE worksheet_block_history SET picture_undo = $2 WHERE id = $1")
                    .bind(history_id)
                    .bind(done.undo.to_json())
                    .execute(db)
                    .await?;
            }
        }
        store::set_version_pictures(db, ws_id, done.version_no, &snapshot).await?;
        return Ok(done.version_no);
    }
    if done.undo.is_empty() {
        return Ok(done.version_no);
    }
    let mut tx = db.begin().await?;
    sqlx::query("SELECT id FROM worksheets WHERE id = $1 FOR UPDATE")
        .bind(ws_id)
        .execute(&mut *tx)
        .await?;
    let latest: (Value,) = sqlx::query_as(
        "SELECT content FROM worksheet_versions WHERE worksheet_id = $1
          ORDER BY version_no DESC LIMIT 1",
    )
    .bind(ws_id)
    .fetch_one(&mut *tx)
    .await?;
    let content: Content = serde_json::from_value(latest.0).unwrap_or_default();
    let saved = store::insert_version_from(
        &mut tx,
        ws_id,
        &content,
        "revise",
        Some(serde_json::json!({
            "cause": "block_proposal_pictures", "proposal_id": proposal.id,
            "interaction_id": proposal.interaction_id, "round": proposal.round,
            "block_ids": [proposal.block_id],
        })),
    )
    .await?;
    store::set_version_pictures(&mut *tx, ws_id, saved.version_no, &snapshot).await?;
    if let Some(block) = content.block(&proposal.block_id) {
        sqlx::query(
            "INSERT INTO worksheet_block_history
               (worksheet_id, block_id, block, interaction_id, proposal_id, version_no,
                after_block, picture_undo)
             VALUES ($1, $2, $3, $4, $5, $6, $3, $7)",
        )
        .bind(ws_id)
        .bind(&proposal.block_id)
        .bind(Json(block))
        .bind(proposal.interaction_id)
        .bind(proposal.id)
        .bind(saved.version_no)
        .bind(done.undo.to_json())
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("UPDATE worksheet_block_proposals SET accepted_version_no = $2 WHERE id = $1")
        .bind(proposal.id)
        .bind(saved.version_no)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(saved.version_no)
}

/// The pictures of an accepted proposal after they ran (see [`record_picture_step`]).
pub struct PicturesDone {
    pub changed: bool,
    pub history_id: Option<i64>,
    pub version_no: i32,
    pub undo: crate::worksheet::images::undo::PictureUndo,
    pub after: crate::worksheet::images::undo::Snapshot,
}

/// 放棄 (or, for a failed or stale one, 知道了): nothing is written; the notes stay open.
pub async fn reject(
    db: &PgPool,
    worksheet_id: i64,
    id: i64,
    teacher_id: i64,
) -> AppResult<Proposal> {
    let p = get(db, worksheet_id, id).await?;
    if p.status == STATUS_RUNNING {
        return Err(AppError::Conflict(
            "Manna 還在改這一區，好了之後再決定。".to_string(),
        ));
    }
    if p.decided_at.is_some() {
        return Err(AppError::Conflict("這個修改已經處理過了。".to_string()));
    }
    sqlx::query(
        "UPDATE worksheet_block_proposals
            SET status = CASE WHEN status = 'proposed' THEN 'rejected' ELSE status END,
                decided_at = now()
          WHERE id = $1 AND decided_at IS NULL",
    )
    .bind(id)
    .execute(db)
    .await?;
    // 放棄 (or 知道了) ends the round: the attempts set aside before can not be adopted any more.
    sqlx::query(
        "UPDATE worksheet_block_proposals SET round_closed_at = now()
          WHERE worksheet_id = $1 AND block_id = $2 AND round_closed_at IS NULL
            AND (set_aside_at IS NOT NULL OR id = $3)",
    )
    .bind(worksheet_id)
    .bind(&p.block_id)
    .bind(id)
    .execute(db)
    .await?;
    if p.status == STATUS_PROPOSED {
        decision(
            db,
            p.interaction_id,
            teacher_id,
            "reject",
            "學習單：放棄逐區修改",
        )
        .await?;
    }
    get(db, worksheet_id, id).await
}
