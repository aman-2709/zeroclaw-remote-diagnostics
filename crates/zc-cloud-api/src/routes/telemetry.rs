//! Telemetry query endpoints.
//!
//! Phase 1: returns empty results (no telemetry storage yet).
//! Phase 2: queries TimescaleDB hypertables.

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
    {
        let devices = state.devices.read().await;
        if !devices.contains_key(&device_id) {
            return Err(ApiError::NotFound(format!(
                "device '{device_id}' not found"
            )));
        }
    }

    // Phase 2: Query TimescaleDB
    Ok(Json(serde_json::json!({
        "device_id": device_id,
        "source": query.source,
        "limit": query.limit,
        "readings": [],
        "message": "telemetry storage not yet implemented (Phase 2)"
    })))
}
