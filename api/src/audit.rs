//! Shared research usage events and scaffold references; independent of HTTP handlers.

use sqlx::PgPool;

/// Latest version of a scaffold by code.
pub async fn scaffold_id(db: &PgPool, code: &str) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar(
        "SELECT id FROM scaffolds WHERE scaffold_code = $1 ORDER BY version DESC LIMIT 1",
    )
    .bind(code)
    .fetch_optional(db)
    .await
}

/// Appends a `usage_logs` row.
pub async fn log_usage<'e, E>(
    executor: E,
    teacher_id: i64,
    event: &str,
    payload: serde_json::Value,
) -> sqlx::Result<()>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query("INSERT INTO usage_logs (teacher_id, event, payload) VALUES ($1, $2, $3)")
        .bind(teacher_id)
        .bind(event)
        .bind(payload)
        .execute(executor)
        .await?;
    Ok(())
}
