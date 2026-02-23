//! Device shadow queries.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// Shadow row returned from the database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ShadowRow {
    pub id: Uuid,
    pub device_id: String,
    pub shadow_name: String,
    pub reported: serde_json::Value,
    pub desired: serde_json::Value,
    pub version: i64,
    pub last_updated: DateTime<Utc>,
}

/// Get a shadow by device ID and shadow name.
pub async fn get_shadow(
    pool: &PgPool,
    device_id: &str,
    shadow_name: &str,
) -> Result<Option<ShadowRow>, sqlx::Error> {
    sqlx::query_as::<_, ShadowRow>(
        "SELECT * FROM device_shadows WHERE device_id = $1 AND shadow_name = $2",
    )
    .bind(device_id)
    .bind(shadow_name)
    .fetch_optional(pool)
    .await
}

/// List all shadows for a device.
pub async fn list_shadows(pool: &PgPool, device_id: &str) -> Result<Vec<ShadowRow>, sqlx::Error> {
    sqlx::query_as::<_, ShadowRow>(
        "SELECT * FROM device_shadows WHERE device_id = $1 ORDER BY shadow_name",
    )
    .bind(device_id)
    .fetch_all(pool)
    .await
}

/// Upsert reported state (JSONB merge via `||`), incrementing version.
pub async fn upsert_reported(
    pool: &PgPool,
    device_id: &str,
    shadow_name: &str,
    reported: &serde_json::Value,
) -> Result<ShadowRow, sqlx::Error> {
    sqlx::query_as::<_, ShadowRow>(
        "INSERT INTO device_shadows (device_id, shadow_name, reported, version, last_updated)
         VALUES ($1, $2, $3, 1, now())
         ON CONFLICT (device_id, shadow_name)
         DO UPDATE SET
             reported = device_shadows.reported || $3,
             version = device_shadows.version + 1,
             last_updated = now()
         RETURNING *",
    )
    .bind(device_id)
    .bind(shadow_name)
    .bind(reported)
    .fetch_one(pool)
    .await
}

/// Set desired state (full replacement), incrementing version.
pub async fn set_desired(
    pool: &PgPool,
    device_id: &str,
    shadow_name: &str,
    desired: &serde_json::Value,
) -> Result<ShadowRow, sqlx::Error> {
    sqlx::query_as::<_, ShadowRow>(
        "INSERT INTO device_shadows (device_id, shadow_name, desired, version, last_updated)
         VALUES ($1, $2, $3, 1, now())
         ON CONFLICT (device_id, shadow_name)
         DO UPDATE SET
             desired = $3,
             version = device_shadows.version + 1,
             last_updated = now()
         RETURNING *",
    )
    .bind(device_id)
    .bind(shadow_name)
    .bind(desired)
    .fetch_one(pool)
    .await
}
