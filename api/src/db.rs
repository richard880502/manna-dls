//! Database connection and embedded migrations.

use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;

pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let opts: PgConnectOptions = database_url.parse()?;
    connect_with(opts, 10).await
}

pub async fn connect_with(
    opts: PgConnectOptions,
    max_connections: u32,
) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(opts)
        .await
}

pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    MIGRATOR.run(pool).await
}

pub async fn ping(pool: &PgPool) -> bool {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(pool)
        .await
        .is_ok()
}
