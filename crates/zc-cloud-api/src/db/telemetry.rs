//! Telemetry reading queries.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Telemetry row returned from the database.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TelemetryRow {
    pub time: DateTime<Utc>,
    pub device_id: String,
    pub metric_name: String,
    pub value_numeric: Option<f64>,
    pub value_text: Option<String>,
    pub value_json: Option<serde_json::Value>,
    pub unit: Option<String>,
    pub source: String,
}

/// Query telemetry readings for a device.
pub async fn query_readings(
    pool: &PgPool,
    device_id: &str,
    source: Option<&str>,
    limit: u32,
) -> Result<Vec<TelemetryRow>, sqlx::Error> {
    if let Some(src) = source {
        sqlx::query_as::<_, TelemetryRow>(
            "SELECT * FROM telemetry_readings
             WHERE device_id = $1 AND source = $2
             ORDER BY time DESC LIMIT $3",
        )
        .bind(device_id)
        .bind(src)
        .bind(limit as i64)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, TelemetryRow>(
            "SELECT * FROM telemetry_readings
             WHERE device_id = $1
             ORDER BY time DESC LIMIT $2",
        )
        .bind(device_id)
        .bind(limit as i64)
        .fetch_all(pool)
        .await
    }
}

/// Insert a batch of telemetry readings.
#[allow(dead_code)]
pub async fn insert_batch(pool: &PgPool, readings: &[TelemetryRow]) -> Result<(), sqlx::Error> {
    for row in readings {
        sqlx::query(
            "INSERT INTO telemetry_readings (time, device_id, metric_name, value_numeric, value_text, value_json, unit, source)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(row.time)
        .bind(&row.device_id)
        .bind(&row.metric_name)
        .bind(row.value_numeric)
        .bind(&row.value_text)
        .bind(&row.value_json)
        .bind(&row.unit)
        .bind(&row.source)
        .execute(pool)
        .await?;
    }
    Ok(())
}
