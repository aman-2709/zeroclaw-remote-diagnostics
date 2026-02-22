//! Device registry queries.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

/// Device row returned from the database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DeviceRow {
    pub id: Uuid,
    pub fleet_id: Uuid,
    pub device_id: String,
    pub status: String,
    pub vin: Option<String>,
    pub hardware_type: String,
    pub certificate_id: Option<String>,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// List all devices.
pub async fn list_all(pool: &PgPool) -> Result<Vec<DeviceRow>, sqlx::Error> {
    sqlx::query_as::<_, DeviceRow>("SELECT * FROM devices ORDER BY device_id")
        .fetch_all(pool)
        .await
}

/// Get a device by its string identifier.
pub async fn get_by_device_id(
    pool: &PgPool,
    device_id: &str,
) -> Result<Option<DeviceRow>, sqlx::Error> {
    sqlx::query_as::<_, DeviceRow>("SELECT * FROM devices WHERE device_id = $1")
        .bind(device_id)
        .fetch_optional(pool)
        .await
}

/// Check if a device exists.
pub async fn exists(pool: &PgPool, device_id: &str) -> Result<bool, sqlx::Error> {
    let row =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM devices WHERE device_id = $1)")
            .bind(device_id)
            .fetch_one(pool)
            .await?;
    Ok(row)
}

/// Insert a new device.
pub async fn insert(pool: &PgPool, row: &DeviceRow) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO devices (id, fleet_id, device_id, status, vin, hardware_type, certificate_id, last_heartbeat, metadata, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(row.id)
    .bind(row.fleet_id)
    .bind(&row.device_id)
    .bind(&row.status)
    .bind(&row.vin)
    .bind(&row.hardware_type)
    .bind(&row.certificate_id)
    .bind(row.last_heartbeat)
    .bind(&row.metadata)
    .bind(row.created_at)
    .bind(row.updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Update the last heartbeat timestamp.
pub async fn update_heartbeat(
    pool: &PgPool,
    device_id: &str,
    heartbeat_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE devices SET last_heartbeat = $1, updated_at = now() WHERE device_id = $2")
        .bind(heartbeat_at)
        .bind(device_id)
        .execute(pool)
        .await?;
    Ok(())
}
