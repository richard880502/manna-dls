//! Teach & Record rules shared by the teacher console, the phone remote and the post-lesson
//! review (spec §4.3, §7.1; contract: docs/API.md「Phase 1.2」). Pure logic only; the routes
//! live in `routes::runs` and `routes::remote`.

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::Value;

/// A QR pairing code is valid for this long.
pub const PAIRING_MINUTES: i64 = 10;
/// After a run ends, a paired phone may still upload bookmarks tapped before the end for this
/// long (offline phones); afterwards its token stops working.
pub const LATE_SYNC_HOURS: i64 = 12;
/// Tolerated difference between a device clock and the server clock when deciding whether a
/// late bookmark was tapped before the run ended.
pub const CLOCK_SKEW_MINUTES: i64 = 5;
/// 「加一句話」 is one sentence.
pub const MAX_NOTE_CHARS: usize = 300;
/// 課後補寫 per bookmark.
pub const MAX_FOLLOWUP_CHARS: usize = 2000;
pub const MAX_OPS_PER_SYNC: usize = 500;
/// Device-generated ids (UUIDs in practice).
pub const MAX_CLIENT_ID_LEN: usize = 64;

pub const POLARITIES: &[&str] = &["good", "stuck"];
pub const DEVICE_TYPES: &[&str] = &["phone", "tablet", "desktop"];
/// 聆聽、筆記、回答、提問、解釋、討論、相互追問、共同完成任務、其他 (canonical order).
pub const BEHAVIORS: &[&str] = &[
    "listening",
    "note_taking",
    "answering",
    "asking",
    "explaining",
    "discussing",
    "probing",
    "co_completing",
    "other",
];
/// 重新解釋、補充例子、調整提問、改變活動、調整時間、稍後處理.
pub const HANDLINGS: &[&str] = &[
    "re_explain",
    "add_example",
    "adjust_questions",
    "change_activity",
    "adjust_time",
    "defer",
];

/// Device event ids and op ids: 1–64 characters of `[A-Za-z0-9_-]`.
pub fn valid_client_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_CLIENT_ID_LEN
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn valid_polarity(s: &str) -> bool {
    POLARITIES.contains(&s)
}

pub fn valid_device_type(s: &str) -> bool {
    DEVICE_TYPES.contains(&s)
}

pub fn valid_handling(s: &str) -> bool {
    HANDLINGS.contains(&s)
}

/// Deduplicates and puts behaviours in canonical order; `Err` names the first unknown value.
pub fn normalize_behaviors(list: &[String]) -> Result<Vec<String>, String> {
    if let Some(bad) = list.iter().find(|b| !BEHAVIORS.contains(&b.as_str())) {
        return Err(bad.clone());
    }
    Ok(BEHAVIORS
        .iter()
        .filter(|b| list.iter().any(|x| x == *b))
        .map(|b| b.to_string())
        .collect())
}

/// Whether a paired remote's token still works at `now` (it never does once revoked).
pub fn remote_token_alive(ended_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    match ended_at {
        None => true,
        Some(end) => now <= end + Duration::hours(LATE_SYNC_HOURS),
    }
}

/// Whether a remote may add a bookmark tapped at `device_ts`: always while the run is open;
/// after the end only within the late-sync window and only if it was tapped before the end
/// (allowing for clock skew).
pub fn remote_create_allowed(
    ended_at: Option<DateTime<Utc>>,
    device_ts: DateTime<Utc>,
    now: DateTime<Utc>,
) -> bool {
    match ended_at {
        None => true,
        Some(end) => {
            remote_token_alive(ended_at, now)
                && device_ts <= end + Duration::minutes(CLOCK_SKEW_MINUTES)
        }
    }
}

/// Who sent an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Teacher,
    Remote { remote_id: i64 },
}

impl Actor {
    pub fn as_str(&self) -> &'static str {
        match self {
            Actor::Teacher => "teacher",
            Actor::Remote { .. } => "remote",
        }
    }
}

/// One device-side operation from the offline queue.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecordOp {
    Create {
        op_id: String,
        device_event_id: String,
        device_ts: DateTime<Utc>,
        polarity: String,
        #[serde(default)]
        note: String,
        device_type: String,
        #[serde(default)]
        via: Option<String>,
    },
    Edit {
        op_id: String,
        device_event_id: String,
        #[serde(default)]
        polarity: Option<String>,
        #[serde(default)]
        note: Option<String>,
        #[serde(default)]
        at: Option<DateTime<Utc>>,
    },
    Undo {
        op_id: String,
        device_event_id: String,
        #[serde(default)]
        at: Option<DateTime<Utc>>,
    },
    Delete {
        op_id: String,
        device_event_id: String,
        #[serde(default)]
        at: Option<DateTime<Utc>>,
    },
}

impl RecordOp {
    pub fn op_id(&self) -> &str {
        match self {
            RecordOp::Create { op_id, .. }
            | RecordOp::Edit { op_id, .. }
            | RecordOp::Undo { op_id, .. }
            | RecordOp::Delete { op_id, .. } => op_id,
        }
    }

    pub fn device_event_id(&self) -> &str {
        match self {
            RecordOp::Create {
                device_event_id, ..
            }
            | RecordOp::Edit {
                device_event_id, ..
            }
            | RecordOp::Undo {
                device_event_id, ..
            }
            | RecordOp::Delete {
                device_event_id, ..
            } => device_event_id,
        }
    }
}

/// A rejected operation: machine code plus a Chinese message for the teacher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub code: &'static str,
    pub message: String,
}

impl Rejection {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Parses one queued operation. A malformed op is rejected on its own so it cannot block the
/// rest of the queue; the op id is echoed back when it can be read.
pub fn parse_op(raw: &Value) -> Result<RecordOp, (String, Rejection)> {
    let op_id = raw
        .get("op_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let op: RecordOp = serde_json::from_value(raw.clone()).map_err(|e| {
        (
            op_id.clone(),
            Rejection::new("bad_request", format!("操作格式不正確：{e}")),
        )
    })?;
    validate_op(&op).map_err(|r| (op_id, r))?;
    Ok(op)
}

/// Field-level checks that do not need the database.
pub fn validate_op(op: &RecordOp) -> Result<(), Rejection> {
    if !valid_client_id(op.op_id()) || !valid_client_id(op.device_event_id()) {
        return Err(Rejection::new(
            "bad_request",
            "op_id 與 device_event_id 需為 1–64 個英數字、- 或 _。",
        ));
    }
    let check_note = |note: &str| {
        if note.chars().count() > MAX_NOTE_CHARS {
            Err(Rejection::new(
                "bad_request",
                format!("一句話最多 {MAX_NOTE_CHARS} 字。"),
            ))
        } else {
            Ok(())
        }
    };
    match op {
        RecordOp::Create {
            polarity,
            note,
            device_type,
            via,
            ..
        } => {
            if !valid_polarity(polarity) {
                return Err(Rejection::new("bad_request", "書籤只能是順利或卡住。"));
            }
            if !valid_device_type(device_type) {
                return Err(Rejection::new("bad_request", "裝置類型不正確。"));
            }
            if let Some(v) = via {
                if !matches!(v.as_str(), "console" | "pip") {
                    return Err(Rejection::new("bad_request", "書籤來源不正確。"));
                }
            }
            check_note(note)
        }
        RecordOp::Edit { polarity, note, .. } => {
            if let Some(p) = polarity {
                if !valid_polarity(p) {
                    return Err(Rejection::new("bad_request", "書籤只能是順利或卡住。"));
                }
            }
            match note {
                Some(n) => check_note(n),
                None => Ok(()),
            }
        }
        RecordOp::Undo { .. } | RecordOp::Delete { .. } => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn client_ids() {
        assert!(valid_client_id("3f2b8c1e-9d4a-4b7e-8f00-1a2b3c4d5e6f"));
        assert!(valid_client_id("a_b-C9"));
        assert!(!valid_client_id(""));
        assert!(!valid_client_id("has space"));
        assert!(!valid_client_id("中文"));
        assert!(!valid_client_id(&"x".repeat(65)));
        assert!(valid_client_id(&"x".repeat(64)));
    }

    #[test]
    fn behaviors_are_deduplicated_in_canonical_order() {
        let input: Vec<String> = ["discussing", "listening", "discussing", "other"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            normalize_behaviors(&input).unwrap(),
            vec!["listening", "discussing", "other"]
        );
        assert_eq!(normalize_behaviors(&[]).unwrap(), Vec::<String>::new());
        assert_eq!(
            normalize_behaviors(&["sleeping".to_string()]).unwrap_err(),
            "sleeping"
        );
    }

    #[test]
    fn remote_tokens_live_until_the_late_sync_window_closes() {
        let end = ts("2026-11-12T03:00:00Z");
        assert!(remote_token_alive(None, end));
        assert!(remote_token_alive(Some(end), end + Duration::hours(11)));
        assert!(remote_token_alive(Some(end), end + Duration::hours(12)));
        assert!(!remote_token_alive(
            Some(end),
            end + Duration::hours(12) + Duration::seconds(1)
        ));
    }

    #[test]
    fn late_remote_bookmarks_must_be_tapped_before_the_end() {
        let end = ts("2026-11-12T03:00:00Z");
        let now = end + Duration::hours(1);
        // open run: anything goes
        assert!(remote_create_allowed(None, end + Duration::days(3), now));
        // tapped before the end, synced later
        assert!(remote_create_allowed(
            Some(end),
            end - Duration::minutes(10),
            now
        ));
        // within clock skew
        assert!(remote_create_allowed(
            Some(end),
            end + Duration::minutes(4),
            now
        ));
        // tapped after the end
        assert!(!remote_create_allowed(
            Some(end),
            end + Duration::minutes(6),
            now
        ));
        // window closed
        assert!(!remote_create_allowed(
            Some(end),
            end - Duration::minutes(10),
            end + Duration::hours(13)
        ));
    }

    #[test]
    fn ops_parse_and_validate() {
        let op = parse_op(&json!({
            "op_id": "op-1", "kind": "create", "device_event_id": "ev-1",
            "device_ts": "2026-11-12T09:41:05.123+08:00", "polarity": "good", "device_type": "phone"
        }))
        .unwrap();
        match &op {
            RecordOp::Create {
                device_ts, note, ..
            } => {
                assert_eq!(*device_ts, ts("2026-11-12T01:41:05.123Z"));
                assert_eq!(note, "");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(op.op_id(), "op-1");
        assert_eq!(op.device_event_id(), "ev-1");

        let (id, r) = parse_op(&json!({ "op_id": "op-2", "kind": "explode" })).unwrap_err();
        assert_eq!(id, "op-2");
        assert_eq!(r.code, "bad_request");

        let (_, r) = parse_op(&json!({
            "op_id": "op-3", "kind": "create", "device_event_id": "ev-3",
            "device_ts": "2026-11-12T01:00:00Z", "polarity": "meh", "device_type": "phone"
        }))
        .unwrap_err();
        assert!(r.message.contains("順利或卡住"));

        let (_, r) = parse_op(&json!({
            "op_id": "op-4", "kind": "edit", "device_event_id": "ev-1",
            "note": "字".repeat(MAX_NOTE_CHARS + 1)
        }))
        .unwrap_err();
        assert!(r.message.contains("一句話"));

        let (_, r) = parse_op(&json!({
            "op_id": "op-5", "kind": "create", "device_event_id": "ev-5",
            "device_ts": "2026-11-12T01:00:00Z", "polarity": "good", "device_type": "watch"
        }))
        .unwrap_err();
        assert!(r.message.contains("裝置類型"));

        let (_, r) = parse_op(&json!({
            "op_id": "op-6", "kind": "create", "device_event_id": "ev-6",
            "device_ts": "2026-11-12T01:00:00Z", "polarity": "good", "device_type": "desktop",
            "via": "remote"
        }))
        .unwrap_err();
        assert!(r.message.contains("來源"));

        assert!(
            parse_op(&json!({ "op_id": "op 7", "kind": "undo", "device_event_id": "ev" })).is_err()
        );
    }
}
