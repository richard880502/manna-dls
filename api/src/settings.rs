//! Platform settings changed by the PI (`app_settings`, key → JSON value).

use serde_json::Value;
use sqlx::PgPool;

pub const ROUTE_CHOICE_ENABLED: &str = "route_choice_enabled";
pub const TEACHER_MEMORY_ENABLED: &str = "teacher_memory_enabled";
/// 社群 (docs/COMMUNITY.md): teachers may share worksheets with each other. Default off.
pub const COMMUNITY_ENABLED: &str = "community_enabled";
/// The PI's override of the text model: `{provider, model, effort, timeout_secs}`. Absent: the
/// environment's `AI_*` values apply.
pub const AI_TEXT_MODEL: &str = "ai_text_model";

pub async fn teacher_memory_enabled(db: &PgPool) -> sqlx::Result<bool> {
    Ok(get(db, TEACHER_MEMORY_ENABLED)
        .await?
        .and_then(|v| v.as_bool())
        .unwrap_or(false))
}

pub async fn community_enabled(db: &PgPool) -> sqlx::Result<bool> {
    Ok(get(db, COMMUNITY_ENABLED)
        .await?
        .and_then(|v| v.as_bool())
        .unwrap_or(false))
}

async fn get(db: &PgPool, key: &str) -> sqlx::Result<Option<Value>> {
    sqlx::query_scalar("SELECT value FROM app_settings WHERE key = $1")
        .bind(key)
        .fetch_optional(db)
        .await
}

/// A stored setting with who changed it (the PI's research id, never a name) and when.
pub struct Stored {
    pub value: Value,
    pub updated_by: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

pub async fn get_stored(db: &PgPool, key: &str) -> sqlx::Result<Option<Stored>> {
    let row: Option<(Value, Option<String>, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT s.value, t.research_id, s.updated_at FROM app_settings s
           LEFT JOIN teachers t ON t.id = s.updated_by WHERE s.key = $1",
    )
    .bind(key)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(value, updated_by, updated_at)| Stored {
        value,
        updated_by,
        updated_at,
    }))
}

/// Removes a setting and returns the value it had (None when it was unset).
pub async fn delete(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    key: &str,
) -> sqlx::Result<Option<Value>> {
    sqlx::query_scalar("DELETE FROM app_settings WHERE key = $1 RETURNING value")
        .bind(key)
        .fetch_optional(&mut **tx)
        .await
}

/// D1: whether teachers may choose among the four prep routes (default true).
pub async fn route_choice_enabled(db: &PgPool) -> sqlx::Result<bool> {
    Ok(get(db, ROUTE_CHOICE_ENABLED)
        .await?
        .and_then(|v| v.as_bool())
        .unwrap_or(true))
}

/// Upserts a setting and returns the previous value (None when it was unset).
pub async fn set(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    key: &str,
    value: &Value,
    updated_by: i64,
) -> sqlx::Result<Option<Value>> {
    let previous: Option<Value> =
        sqlx::query_scalar("SELECT value FROM app_settings WHERE key = $1 FOR UPDATE")
            .bind(key)
            .fetch_optional(&mut **tx)
            .await?;
    sqlx::query(
        "INSERT INTO app_settings (key, value, updated_by, updated_at) VALUES ($1, $2, $3, now())
         ON CONFLICT (key) DO UPDATE
           SET value = EXCLUDED.value, updated_by = EXCLUDED.updated_by, updated_at = now()",
    )
    .bind(key)
    .bind(value)
    .bind(updated_by)
    .execute(&mut **tx)
    .await?;
    Ok(previous)
}
