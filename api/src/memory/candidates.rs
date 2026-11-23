//! A candidate is a habit Manna proposes and the teacher has not decided on yet: from a preview
//! on 我的出題習慣, or from something the teacher typed in a conversation (docs/TEACHER_MEMORY.md
//! 「對話中偵測」). Nothing becomes a memory without [`confirm`]. A candidate lives 15 minutes.
//! Every step is a research event; none carries the teacher's text.
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use super::extraction::Proposed;
use super::{dirty, selection, IndexChange, CUSTOM, MAX_ACTIVE};
use crate::audit::log_usage;
use crate::error::{AppError, AppResult};
use crate::AppState;

/// How long a candidate can still be confirmed.
pub const LIFETIME: &str = "15 minutes";

/// Where the text came from. Only ids; the text itself is never kept with a candidate.
#[derive(Debug, Clone)]
pub enum Source {
    /// The preview on 我的出題習慣.
    Manual,
    /// A message of a 備課對話 (the home quick box starts one with `entry = home`).
    PrepTurn {
        session_id: i64,
        turn_id: i64,
        entry: Option<&'static str>,
    },
    /// The instruction of 逐區修改.
    BlockRevise { worksheet_id: i64, proposal_id: i64 },
    /// 「教師的要求」 of a worksheet.
    WorksheetRequest { worksheet_id: i64 },
}

impl Source {
    pub fn code(&self) -> &'static str {
        match self {
            Source::Manual => "manual",
            Source::PrepTurn { .. } => "prep_turn",
            Source::BlockRevise { .. } => "block_revise",
            Source::WorksheetRequest { .. } => "worksheet_request",
        }
    }

    /// What the extraction model is told about where the text was typed.
    pub fn where_said(&self) -> &'static str {
        match self {
            Source::Manual => "老師在「我的出題習慣」頁直接寫下想記住的習慣。",
            Source::PrepTurn { .. } => "老師在備課對話中打給 Manna 的一段話（可能只針對這一次）。",
            Source::BlockRevise { .. } => {
                "老師在學習單某一區按「請 Manna 修改」時打的修改指示（多半只針對這一區）。"
            }
            Source::WorksheetRequest { .. } => "老師寫在學習單上的「教師的要求」。",
        }
    }

    /// `input_snapshot.source` of the extraction interaction.
    pub fn snapshot(&self) -> Value {
        match self {
            Source::Manual => json!({"kind": "manual"}),
            Source::PrepTurn {
                session_id,
                turn_id,
                entry,
            } => json!({"kind": "prep_turn", "session_id": session_id, "turn_id": turn_id,
                "entry": entry}),
            Source::BlockRevise {
                worksheet_id,
                proposal_id,
            } => json!({"kind": "block_revise", "worksheet_id": worksheet_id,
                "proposal_id": proposal_id}),
            Source::WorksheetRequest { worksheet_id } => {
                json!({"kind": "worksheet_request", "worksheet_id": worksheet_id})
            }
        }
    }

    fn links(&self) -> (Option<i64>, Option<i64>, Option<i64>, Option<i64>) {
        match self {
            Source::Manual => (None, None, None, None),
            Source::PrepTurn {
                session_id,
                turn_id,
                ..
            } => (Some(*session_id), Some(*turn_id), None, None),
            Source::BlockRevise {
                worksheet_id,
                proposal_id,
            } => (None, None, Some(*worksheet_id), Some(*proposal_id)),
            Source::WorksheetRequest { worksheet_id } => (None, None, Some(*worksheet_id), None),
        }
    }
}

/// Store a candidate and, for one that came from a conversation, log that it was detected
/// (source, kind and scope only).
pub async fn create(
    db: &PgPool,
    teacher_id: i64,
    p: &Proposed,
    interaction_id: i64,
    source: &Source,
    digest: Option<&str>,
) -> sqlx::Result<i64> {
    let (session, turn, worksheet, proposal) = source.links();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO memory_candidates
         (teacher_id, kind, value, statement, scope, replaces_id, interaction_id, source,
          prep_session_id, prep_turn_id, worksheet_id, proposal_id, source_digest)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) RETURNING id",
    )
    .bind(teacher_id)
    .bind(&p.kind)
    .bind(&p.value)
    .bind(&p.statement)
    .bind(&p.scope)
    .bind(p.replaces_id)
    .bind(interaction_id)
    .bind(source.code())
    .bind(session)
    .bind(turn)
    .bind(worksheet)
    .bind(proposal)
    .bind(digest)
    .fetch_one(db)
    .await?;
    if !matches!(source, Source::Manual) {
        log_usage(
            db,
            teacher_id,
            "memory_candidate_detected",
            json!({"candidate_id": id, "source": source.code(), "kind": p.kind,
                "scope": p.scope, "replaces": p.replaces_id.is_some(),
                "interaction_id": interaction_id, "session_id": session, "turn_id": turn,
                "worksheet_id": worksheet, "proposal_id": proposal}),
        )
        .await?;
    }
    Ok(id)
}

/// A pending suggestion as the page shows it.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct CandidateView {
    pub id: i64,
    pub kind: String,
    pub scope: Option<String>,
    pub statement: String,
    pub replaces_id: Option<i64>,
    pub replaces_statement: Option<String>,
    pub source: String,
    pub prep_session_id: Option<i64>,
    pub prep_turn_id: Option<i64>,
    pub worksheet_id: Option<i64>,
    pub proposal_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    /// The first suggestion this teacher sees: the page adds the one-time explanation.
    pub intro: bool,
}

#[derive(Debug, Default)]
pub struct Filter {
    pub prep_session_id: Option<i64>,
    pub worksheet_id: Option<i64>,
}

/// Candidates nobody decided on in time: a decision `no_response` and a usage event, once.
pub async fn sweep_expired(db: &PgPool, teacher_id: i64) -> sqlx::Result<()> {
    let expired: Vec<(i64, String, String, Option<String>, i64)> = sqlx::query_as(&format!(
        "UPDATE memory_candidates SET expiry_logged_at = clock_timestamp()
         WHERE teacher_id = $1 AND confirmed_at IS NULL AND cancelled_at IS NULL
           AND expiry_logged_at IS NULL AND source <> 'manual'
           AND created_at <= clock_timestamp() - interval '{LIFETIME}'
         RETURNING id, source, kind, scope, interaction_id"
    ))
    .bind(teacher_id)
    .fetch_all(db)
    .await?;
    for (id, source, kind, scope, interaction_id) in expired {
        sqlx::query(
            "INSERT INTO teacher_decisions (ai_interaction_id, teacher_id, item_index, decision, reason_note)
             VALUES ($1, $2, 0, 'no_response', 'expired')",
        )
        .bind(interaction_id)
        .bind(teacher_id)
        .execute(db)
        .await?;
        log_usage(
            db,
            teacher_id,
            "memory_candidate_expired",
            json!({"candidate_id": id, "source": source, "kind": kind, "scope": scope}),
        )
        .await?;
    }
    Ok(())
}

/// The teacher's pending suggestions from conversations, oldest first.
pub async fn pending(
    db: &PgPool,
    teacher_id: i64,
    filter: &Filter,
) -> sqlx::Result<Vec<CandidateView>> {
    sweep_expired(db, teacher_id).await?;
    sqlx::query_as(&format!(
        "SELECT c.id, c.kind, c.scope, c.statement, c.replaces_id, r.statement AS replaces_statement,
                c.source, c.prep_session_id, c.prep_turn_id, c.worksheet_id, c.proposal_id,
                c.created_at, c.created_at + interval '{LIFETIME}' AS expires_at,
                NOT EXISTS (SELECT 1 FROM memory_candidates o
                             WHERE o.teacher_id = c.teacher_id AND o.source <> 'manual'
                               AND o.shown_at IS NOT NULL AND o.id < c.id) AS intro
           FROM memory_candidates c
           LEFT JOIN teacher_memories r ON r.id = c.replaces_id AND r.invalid_at IS NULL
          WHERE c.teacher_id = $1 AND c.source <> 'manual'
            AND c.confirmed_at IS NULL AND c.cancelled_at IS NULL
            AND c.created_at > clock_timestamp() - interval '{LIFETIME}'
            AND ($2::bigint IS NULL OR c.prep_session_id = $2)
            AND ($3::bigint IS NULL OR c.worksheet_id = $3)
          ORDER BY c.id"
    ))
    .bind(teacher_id)
    .bind(filter.prep_session_id)
    .bind(filter.worksheet_id)
    .fetch_all(db)
    .await
}

/// The page displayed the suggestion (logged once).
pub async fn mark_shown(db: &PgPool, teacher_id: i64, id: i64) -> AppResult<()> {
    type Shown = (String, String, Option<String>, Option<i64>, Option<i64>);
    let row: Option<Shown> = sqlx::query_as(
        "UPDATE memory_candidates SET shown_at = clock_timestamp()
         WHERE id = $1 AND teacher_id = $2 AND shown_at IS NULL AND source <> 'manual'
           AND confirmed_at IS NULL AND cancelled_at IS NULL
         RETURNING source, kind, scope, prep_turn_id, proposal_id",
    )
    .bind(id)
    .bind(teacher_id)
    .fetch_optional(db)
    .await?;
    match row {
        Some((source, kind, scope, turn, proposal)) => {
            log_usage(
                db,
                teacher_id,
                "memory_candidate_shown",
                json!({"candidate_id": id, "source": source, "kind": kind, "scope": scope,
                    "turn_id": turn, "proposal_id": proposal}),
            )
            .await?;
            Ok(())
        }
        None => {
            let exists: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM memory_candidates WHERE id = $1 AND teacher_id = $2",
            )
            .bind(id)
            .bind(teacher_id)
            .fetch_optional(db)
            .await?;
            exists
                .map(|_| ())
                .ok_or_else(|| AppError::not_found("找不到這個建議。"))
        }
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    kind: String,
    value: String,
    statement: String,
    scope: Option<String>,
    replaces_id: Option<i64>,
    interaction_id: i64,
    source: String,
}

/// The teacher's decision on the habit-sorting suggestion, like other AI suggestions.
async fn decide(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    interaction_id: i64,
    teacher_id: i64,
    decision: &str,
    note: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO teacher_decisions (ai_interaction_id, teacher_id, item_index, decision, reason_note)
         VALUES ($1, $2, 0, $3, $4)",
    )
    .bind(interaction_id)
    .bind(teacher_id)
    .bind(decision)
    .bind(note)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub struct Confirmed {
    pub id: i64,
    /// Exactly what was already active: no new row, no re-index.
    pub unchanged: bool,
}

/// The teacher said 記住. Same transaction for the limit, the replacement, the history and the
/// index operations; a candidate that expired, was decided on or no longer matches the habits
/// now is a 409 and the teacher starts again.
pub async fn confirm(state: &AppState, teacher_id: i64, candidate_id: i64) -> AppResult<Confirmed> {
    let mut tx = state.db.begin().await?;
    // Serialize all confirms/removals for this teacher, including the limit and replacements.
    sqlx::query("SELECT id FROM teachers WHERE id=$1 FOR UPDATE")
        .bind(teacher_id)
        .execute(&mut *tx)
        .await?;
    let c: Row = sqlx::query_as(&format!(
        "SELECT kind,value,statement,scope,replaces_id,interaction_id,source
        FROM memory_candidates WHERE id=$1 AND teacher_id=$2 AND confirmed_at IS NULL
        AND cancelled_at IS NULL AND created_at > clock_timestamp() - interval '{LIFETIME}'
        FOR UPDATE"
    ))
    .bind(candidate_id)
    .bind(teacher_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::Conflict("預覽已過期或已確認，請重新整理習慣。".into()))?;
    let custom = c.kind == CUSTOM;
    // What is active now: the closed kind's single habit, or the same sentence and scope.
    let current: Option<(i64, bool)> = if custom {
        sqlx::query_as(
            "SELECT id, true FROM teacher_memories
            WHERE teacher_id=$1 AND kind='custom' AND invalid_at IS NULL
              AND statement=$2 AND scope IS NOT DISTINCT FROM $3",
        )
        .bind(teacher_id)
        .bind(&c.statement)
        .bind(&c.scope)
        .fetch_optional(&mut *tx)
        .await?
    } else {
        sqlx::query_as(
            "SELECT id, value = $3 FROM teacher_memories
            WHERE teacher_id=$1 AND kind=$2 AND invalid_at IS NULL",
        )
        .bind(teacher_id)
        .bind(&c.kind)
        .bind(&c.value)
        .fetch_optional(&mut *tx)
        .await?
    };
    let at: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await?;
    // Confirming exactly what is already active changes nothing: no new row, no re-index.
    if let Some((same, _)) = current.filter(|(_, same_value)| *same_value) {
        sqlx::query("UPDATE memory_candidates SET confirmed_at=$2 WHERE id=$1")
            .bind(candidate_id)
            .bind(at)
            .execute(&mut *tx)
            .await?;
        decide(
            &mut tx,
            c.interaction_id,
            teacher_id,
            "adopt",
            "already_active",
        )
        .await?;
        log_usage(
            &mut *tx,
            teacher_id,
            "memory_confirmed",
            json!({"candidate_id": candidate_id, "memory_id": same, "kind": c.kind,
                "value": closed_value(&c), "scope": c.scope, "source": c.source,
                "unchanged": true}),
        )
        .await?;
        tx.commit().await?;
        return Ok(Confirmed {
            id: same,
            unchanged: true,
        });
    }
    // The habit this one replaces: the closed kind's current one, or the free-form one named.
    let target: Option<i64> = if custom {
        if let Some(id) = c.replaces_id {
            let active: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM teacher_memories
                WHERE id=$1 AND teacher_id=$2 AND kind='custom' AND invalid_at IS NULL",
            )
            .bind(id)
            .bind(teacher_id)
            .fetch_optional(&mut *tx)
            .await?;
            if active.is_none() {
                return Err(AppError::Conflict(
                    "習慣已變更，請重新整理並確認取代的項目。".into(),
                ));
            }
            active
        } else {
            None
        }
    } else {
        let now = current.map(|(id, _)| id);
        if now != c.replaces_id {
            return Err(AppError::Conflict(
                "習慣已變更，請重新整理並確認取代的項目。".into(),
            ));
        }
        now
    };
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM teacher_memories WHERE teacher_id=$1 AND invalid_at IS NULL",
    )
    .bind(teacher_id)
    .fetch_one(&mut *tx)
    .await?;
    if target.is_none() && count >= MAX_ACTIVE {
        return Err(AppError::Conflict(format!(
            "最多記住 {MAX_ACTIVE} 項習慣，請先移除一項。"
        )));
    }
    let id: i64 = sqlx::query_scalar("SELECT nextval('teacher_memories_id_seq')")
        .fetch_one(&mut *tx)
        .await?;
    if let Some(old) = target {
        sqlx::query("UPDATE teacher_memories SET invalid_at=$2,superseded_by=$3,invalidation_reason='replaced' WHERE id=$1")
            .bind(old).bind(at).bind(id).execute(&mut *tx).await?;
    }
    sqlx::query(
        "INSERT INTO teacher_memories (id,teacher_id,kind,value,statement,scope,valid_at,interaction_id)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(id)
    .bind(teacher_id)
    .bind(&c.kind)
    .bind(&c.value)
    .bind(&c.statement)
    .bind(&c.scope)
    .bind(at)
    .bind(c.interaction_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE memory_candidates SET confirmed_at=$2 WHERE id=$1")
        .bind(candidate_id)
        .bind(at)
        .execute(&mut *tx)
        .await?;
    decide(&mut tx, c.interaction_id, teacher_id, "adopt", "confirmed").await?;
    log_usage(
        &mut *tx,
        teacher_id,
        "memory_confirmed",
        json!({"candidate_id": candidate_id, "memory_id": id, "replaces_id": target,
            "kind": c.kind, "value": closed_value(&c), "scope": c.scope, "source": c.source,
            "unchanged": false}),
    )
    .await?;
    let mut changes = vec![IndexChange::Add(id)];
    if let Some(old) = target {
        changes.push(IndexChange::Remove(old));
    }
    dirty(&mut tx, &state.config.memory, teacher_id, &changes).await?;
    tx.commit().await?;
    if custom {
        selection::embed_in_background(state, id, c.statement);
    }
    Ok(Confirmed {
        id,
        unchanged: false,
    })
}

/// The closed value for the research event; a free-form habit's sentence is never logged there.
fn closed_value(c: &Row) -> Option<&str> {
    (c.kind != CUSTOM).then_some(c.value.as_str())
}

/// How the teacher closed a candidate without remembering it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dismissal {
    /// 取消 / 不用.
    Cancel,
    /// 修改後記住: the suggestion is replaced by what the teacher edits.
    Edit,
}

/// The candidate can no longer be confirmed; the decision is recorded (reject, or revise for an
/// edit).
pub async fn dismiss(
    state: &AppState,
    teacher_id: i64,
    candidate_id: i64,
    how: Dismissal,
) -> AppResult<()> {
    let mut tx = state.db.begin().await?;
    let row: Option<(i64, String, String, Option<String>)> = sqlx::query_as(
        "UPDATE memory_candidates SET cancelled_at=clock_timestamp()
        WHERE id=$1 AND teacher_id=$2 AND confirmed_at IS NULL AND cancelled_at IS NULL
        RETURNING interaction_id, source, kind, scope",
    )
    .bind(candidate_id)
    .bind(teacher_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (interaction_id, source, kind, scope) =
        row.ok_or_else(|| AppError::Conflict("這個預覽已確認或已取消。".into()))?;
    let from_conversation = source != "manual";
    let (decision, note, event) = match (how, from_conversation) {
        (Dismissal::Edit, _) => (
            "revise",
            "edit_requested",
            "memory_candidate_edit_requested",
        ),
        (Dismissal::Cancel, true) => ("reject", "dismissed", "memory_candidate_dismissed"),
        (Dismissal::Cancel, false) => ("reject", "cancelled", "memory_cancelled"),
    };
    decide(&mut tx, interaction_id, teacher_id, decision, note).await?;
    log_usage(
        &mut *tx,
        teacher_id,
        event,
        json!({"candidate_id": candidate_id, "source": source, "kind": kind, "scope": scope}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
