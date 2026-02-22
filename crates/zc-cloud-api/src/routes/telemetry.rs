//! Telemetry query endpoints.
//!
//! Queries the telemetry_readings table when database is available,
//! otherwise returns empty results.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Query parameters for telemetry requests.
#[derive(Debug, Deserialize)]
pub struct TelemetryQuery {
    /// Filter by telemetry source (obd2, system, canbus).
    pub source: Option<String>,
    /// Maximum number of results.
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    100
}

/// GET /api/v1/devices/:id/telemetry — query device telemetry.
pub async fn get_telemetry(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    Query(query): Query<TelemetryQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    // Verify device exists
    if let Some(pool) = &state.pool {
        let exists = crate::db::devices::exists(pool, &device_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        if !exists {
            return Err(ApiError::NotFound(format!(
                "device '{device_id}' not found"
            )));
        }

        // Query real telemetry data
        let rows = crate::db::telemetry::query_readings(
            pool,
            &device_id,
            query.source.as_deref(),
            query.limit,
        )
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

        let readings: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "time": r.time,
                    "metric_name": r.metric_name,
                    "value_numeric": r.value_numeric,
                    "value_text": r.value_text,
                    "value_json": r.value_json,
                    "unit": r.unit,
                    "source": r.source,
                })
            })
            .collect();

        return Ok(Json(serde_json::json!({
            "device_id": device_id,
            "source": query.source,
            "limit": query.limit,
            "readings": readings,
        })));
    }

    // In-memory fallback: verify device exists, return empty readings
    {
        let devices = state.devices.read().await;
        if !devices.contains_key(&device_id) {
            return Err(ApiError::NotFound(format!(
                "device '{device_id}' not found"
            )));
        }
    }

    Ok(Json(serde_json::json!({
        "device_id": device_id,
        "source": query.source,
        "limit": query.limit,
        "readings": [],
        "message": "telemetry storage not yet implemented (in-memory mode)"
    })))
}
