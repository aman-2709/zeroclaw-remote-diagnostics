//! Command dispatch and response queries.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// Command row returned from the database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CommandRow {
    pub id: Uuid,
    pub fleet_id: String,
    pub device_id: String,
    pub natural_language: String,
    pub initiated_by: String,
    pub correlation_id: Uuid,
    pub timeout_secs: i32,

    // Parsed intent
    pub tool_name: Option<String>,
    pub tool_args: Option<serde_json::Value>,
    pub confidence: Option<f64>,

    // Response
    pub status: String,
    pub inference_tier: Option<String>,
    pub response_text: Option<String>,
    pub response_data: Option<serde_json::Value>,
    pub latency_ms: Option<i64>,
    pub responded_at: Option<DateTime<Utc>>,
    pub error: Option<String>,

    pub created_at: DateTime<Utc>,
}

/// Insert a new command (status = 'pending').
pub async fn insert(pool: &PgPool, row: &CommandRow) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO commands (id, fleet_id, device_id, natural_language, initiated_by, correlation_id, timeout_secs, status, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(row.id)
    .bind(&row.fleet_id)
    .bind(&row.device_id)
    .bind(&row.natural_language)
    .bind(&row.initiated_by)
    .bind(row.correlation_id)
    .bind(row.timeout_secs)
    .bind(&row.status)
    .bind(row.created_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get a command by ID.
pub async fn get_by_id(pool: &PgPool, command_id: Uuid) -> Result<Option<CommandRow>, sqlx::Error> {
    sqlx::query_as::<_, CommandRow>("SELECT * FROM commands WHERE id = $1")
        .bind(command_id)
        .fetch_optional(pool)
        .await
}

/// List recent commands (most recent first).
pub async fn list_recent(pool: &PgPool, limit: i64) -> Result<Vec<CommandRow>, sqlx::Error> {
    sqlx::query_as::<_, CommandRow>("SELECT * FROM commands ORDER BY created_at DESC LIMIT $1")
        .bind(limit)
        .fetch_all(pool)
        .await
}

/// Update command with a response.
#[allow(dead_code, clippy::too_many_arguments)]
pub async fn update_response(
    pool: &PgPool,
    command_id: Uuid,
    status: &str,
    inference_tier: &str,
    response_text: Option<&str>,
    response_data: Option<&serde_json::Value>,
    latency_ms: i64,
    error: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE commands SET status = $1, inference_tier = $2, response_text = $3,
         response_data = $4, latency_ms = $5, responded_at = now(), error = $6
         WHERE id = $7",
    )
    .bind(status)
    .bind(inference_tier)
    .bind(response_text)
    .bind(response_data)
    .bind(latency_ms)
    .bind(error)
    .bind(command_id)
    .execute(pool)
    .await?;
    Ok(())
}
