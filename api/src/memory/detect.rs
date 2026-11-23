//! Noticing a lasting preference in what the teacher types to Manna, so that she can be asked
//! 「要記住這個習慣嗎？」 with one tap. Never silent: this only proposes a candidate
//! ([`super::candidates`]); remembering needs her confirmation.
//!
//! Only text the teacher typed as an instruction is looked at (a message, a revise instruction,
//! the worksheet request) and only while the PI's research switch is on. A cheap deterministic
//! prefilter ([`looks_persistent`]) decides whether the text goes to the model at all. The model
//! call runs in the background with the model pinned for the work it came with, and is logged
//! as its own `ai_interactions` row with the source.
use sha2::{Digest, Sha256};
use std::future::Future;
use std::pin::Pin;

use super::candidates::{self, Source};
use super::extraction::{self, MAX_INPUT_CHARS};
use super::{list, CUSTOM, MAX_ACTIVE};
use crate::audit::log_usage;
use crate::error::AppResult;
use crate::AppState;

/// Suggestions waiting for an answer, per teacher: past this nothing new is proposed.
const MAX_PENDING: i64 = 5;

/// Phrases that signal a lasting preference (lowercase; the text is lowercased first).
const MARKERS: &[&str] = &[
    // 中文
    "之後",
    "以後",
    "往後",
    "日後",
    "從現在起",
    "從現在開始",
    "從今以後",
    "每次",
    "每份",
    "每一份",
    "每個學習單",
    "都要",
    "一律",
    "固定",
    "總是",
    "永遠",
    "一直",
    "我習慣",
    "我喜歡",
    "我偏好",
    "我通常",
    "我的學生都",
    "我的學生總是",
    "不要再",
    "別再",
    "再也不要",
    "不要總是",
    // English
    "from now on",
    "going forward",
    "in the future",
    "every time",
    "each time",
    "whenever",
    "always",
    "never",
    "i prefer",
    "i like",
    "i usually",
    "i always",
    "don't ever",
    "do not ever",
    "stop using",
    "no longer",
];

/// Whether the text may state a lasting preference and is worth asking the model about. A miss
/// costs nothing; a hit still has to pass the model (a one-time request is `persistent=false`).
pub fn looks_persistent(text: &str) -> bool {
    let text = text.trim();
    if text.chars().count() < 4 {
        return false;
    }
    let lower = text.to_lowercase();
    MARKERS.iter().any(|m| lower.contains(m))
}

/// Digest of the text (whitespace-normalised): the same words are not asked about twice.
fn digest(text: &str) -> String {
    let norm = text.split_whitespace().collect::<Vec<_>>().join(" ");
    hex::encode(Sha256::digest(norm.as_bytes()))
}

type Job = Pin<Box<dyn Future<Output = AppResult<()>> + Send>>;

/// Look at what the teacher typed, in the background. Returns at once; never fails the request
/// it hangs off. Does nothing unless the research switch is on, AI is on, and the text passes
/// the prefilter.
pub async fn spawn(state: &AppState, teacher_id: i64, source: Source, text: &str) {
    if !looks_persistent(text) {
        return;
    }
    match crate::settings::teacher_memory_enabled(&state.db).await {
        Ok(true) => {}
        _ => return,
    }
    let text: String = text.trim().chars().take(MAX_INPUT_CHARS).collect();
    // The model the work (turn, revision) started with, even if the PI switches meanwhile.
    let work = {
        let state = state.clone();
        let source = source.clone();
        async move { detect(&state, teacher_id, &source, &text).await }
    };
    let job: Job = match &source {
        Source::PrepTurn { turn_id, .. } => Box::pin(state.ai.pinned_turn(*turn_id, work)),
        _ => Box::pin(state.ai.pinned(work)),
    };
    tokio::spawn(async move {
        // A panic or failure stays here: the teacher's request has already been answered.
        let _ = crate::jobs::run("memory_detect", teacher_id, job).await;
    });
}

/// The detection itself (awaited by tests): extract, check and store at most one candidate.
pub async fn detect(
    state: &AppState,
    teacher_id: i64,
    source: &Source,
    text: &str,
) -> AppResult<()> {
    let db = &state.db;
    if !state.ai.active().enabled() {
        return Ok(());
    }
    candidates::sweep_expired(db, teacher_id).await?;
    let digest = digest(text);
    // The same words, asked about in the last quarter of an hour (再試一次, 再改一次).
    let repeated: bool = sqlx::query_scalar(&format!(
        "SELECT EXISTS (SELECT 1 FROM memory_candidates WHERE teacher_id=$1 AND source_digest=$2
          AND created_at > clock_timestamp() - interval '{}')",
        candidates::LIFETIME
    ))
    .bind(teacher_id)
    .bind(&digest)
    .fetch_one(db)
    .await?;
    let pending: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM memory_candidates WHERE teacher_id=$1 AND source <> 'manual'
          AND confirmed_at IS NULL AND cancelled_at IS NULL
          AND created_at > clock_timestamp() - interval '{}'",
        candidates::LIFETIME
    ))
    .bind(teacher_id)
    .fetch_one(db)
    .await?;
    if repeated || pending >= MAX_PENDING {
        return Ok(());
    }
    let outcome = extraction::run(state, teacher_id, text, source).await?;
    let mut verdict = if outcome.proposed.is_some() {
        "candidate"
    } else if outcome.reasons.contains(&"not_persistent") {
        "not_persistent"
    } else if outcome.status == "shown" || outcome.status == "blocked" {
        "refused"
    } else {
        "failed"
    };
    if let Some(p) = &outcome.proposed {
        let active = list(db, teacher_id, false).await?;
        let already = active.iter().any(|m| {
            m.kind == p.kind
                && if p.kind == CUSTOM {
                    m.statement == p.statement && m.scope == p.scope
                } else {
                    m.value == p.value
                }
        });
        let full = p.replaces_id.is_none()
            && !already
            && active.len() as i64 >= MAX_ACTIVE
            && (p.kind == CUSTOM || !active.iter().any(|m| m.kind == p.kind));
        if already {
            verdict = "already_active";
        } else if full {
            verdict = "limit_reached";
        } else {
            candidates::create(
                db,
                teacher_id,
                p,
                outcome.interaction_id,
                source,
                Some(&digest),
            )
            .await?;
        }
    }
    log_usage(
        db,
        teacher_id,
        "memory_detection_run",
        serde_json::json!({"source": source.code(), "verdict": verdict,
            "reasons": outcome.reasons, "interaction_id": outcome.interaction_id}),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prefilter_lets_through_lasting_preferences_only() {
        for hit in [
            "之後選擇題都用三個選項",
            "以後每份學習單都附反思問題",
            "我習慣題目先放短文",
            "從現在起題幹不要用 Which of the following",
            "不要再出填充題了",
            "From now on use three options",
            "I always want a vocabulary box",
            "Every time, add a warm-up question",
            "我的學生都不太會讀長句，題目要短",
        ] {
            assert!(looks_persistent(hit), "{hit}");
        }
        for miss in [
            "這次改成三題",
            "第 2 題太難，換簡單一點",
            "把第一區的題目改成配對題",
            "請幫我設計一份閱讀學習單",
            "Make question 2 easier",
            "好",
            "",
        ] {
            assert!(!looks_persistent(miss), "{miss}");
        }
    }

    #[test]
    fn the_digest_ignores_spacing() {
        assert_eq!(digest("之後  選擇題\n三個"), digest("之後 選擇題 三個"));
        assert_ne!(digest("之後選擇題三個"), digest("之後選擇題四個"));
    }
}
