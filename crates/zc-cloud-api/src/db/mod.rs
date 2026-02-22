//! Database access layer for PostgreSQL.
//!
//! Each sub-module provides typed query functions over a `PgPool`.

pub mod commands;
pub mod devices;
pub mod telemetry;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// Connect to PostgreSQL and run migrations.
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;

    tracing::info!("running database migrations");
    sqlx::raw_sql(include_str!("../../migrations/001_devices.sql"))
        .execute(&pool)
        .await?;
    sqlx::raw_sql(include_str!("../../migrations/002_commands.sql"))
        .execute(&pool)
        .await?;
    sqlx::raw_sql(include_str!("../../migrations/003_telemetry.sql"))
        .execute(&pool)
        .await?;
    sqlx::raw_sql(include_str!("../../migrations/004_heartbeats.sql"))
        .execute(&pool)
        .await?;
    tracing::info!("migrations complete");

    Ok(pool)
}
