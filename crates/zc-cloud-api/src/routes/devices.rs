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
pub async fn list_devices(State(state): State<AppState>) -> ApiResult<Json<Vec<DeviceSummary>>> {
    if let Some(pool) = &state.pool {
        let rows = crate::db::devices::list_all(pool)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        let summaries = rows
            .into_iter()
            .map(|r| DeviceSummary {
                device_id: r.device_id,
                status: parse_device_status(&r.status),
                hardware_type: parse_hardware_type(&r.hardware_type),
                last_heartbeat: r.last_heartbeat,
            })
            .collect();
        return Ok(Json(summaries));
    }

    // In-memory fallback
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
    Ok(Json(summaries))
}

/// GET /api/v1/devices/:id — get device details.
pub async fn get_device(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
) -> ApiResult<Json<DeviceInfo>> {
    if let Some(pool) = &state.pool {
        let row = crate::db::devices::get_by_device_id(pool, &device_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound(format!("device '{device_id}' not found")))?;
        return Ok(Json(row_to_device_info(row)));
    }

    // In-memory fallback
    let devices = state.devices.read().await;
    devices
        .get(&device_id)
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("device '{device_id}' not found")))
}

fn parse_device_status(s: &str) -> DeviceStatus {
    match s {
        "online" => DeviceStatus::Online,
        "offline" => DeviceStatus::Offline,
        "maintenance" => DeviceStatus::Maintenance,
        "decommissioned" => DeviceStatus::Decommissioned,
        _ => DeviceStatus::Provisioning,
    }
}

fn parse_hardware_type(s: &str) -> HardwareType {
    match s {
        "raspberry_pi_4" => HardwareType::RaspberryPi4,
        "raspberry_pi_5" => HardwareType::RaspberryPi5,
        "industrial_sbc" => HardwareType::IndustrialSbc,
        _ => HardwareType::Custom(s.to_string()),
    }
}

fn row_to_device_info(r: crate::db::devices::DeviceRow) -> DeviceInfo {
    DeviceInfo {
        id: r.id,
        fleet_id: zc_protocol::device::FleetId(r.fleet_id),
        device_id: r.device_id,
        status: parse_device_status(&r.status),
        vin: r.vin,
        hardware_type: parse_hardware_type(&r.hardware_type),
        certificate_id: r.certificate_id,
        last_heartbeat: r.last_heartbeat,
        metadata: r.metadata,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}
