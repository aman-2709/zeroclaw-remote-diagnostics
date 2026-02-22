//! Device registry endpoints.

use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use zc_protocol::device::{DeviceInfo, DeviceStatus, HardwareType};

/// Summary view of a device (for list responses).
#[derive(Debug, Serialize)]
pub struct DeviceSummary {
    pub device_id: String,
    pub status: DeviceStatus,
    pub hardware_type: HardwareType,
    pub last_heartbeat: Option<chrono::DateTime<chrono::Utc>>,
}

/// GET /api/v1/devices — list all devices.
pub async fn list_devices(State(state): State<AppState>) -> Json<Vec<DeviceSummary>> {
    let devices = state.devices.read().await;
    let summaries: Vec<DeviceSummary> = devices
        .values()
        .map(|d| DeviceSummary {
            device_id: d.device_id.clone(),
            status: d.status,
            hardware_type: d.hardware_type.clone(),
            last_heartbeat: d.last_heartbeat,
        })
        .collect();
    Json(summaries)
}

/// GET /api/v1/devices/:id — get device details.
pub async fn get_device(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
) -> ApiResult<Json<DeviceInfo>> {
    let devices = state.devices.read().await;
    devices
        .get(&device_id)
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("device '{device_id}' not found")))
}
