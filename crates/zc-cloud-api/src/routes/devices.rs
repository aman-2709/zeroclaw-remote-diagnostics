//! Device registry endpoints.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::events::WsEvent;
use crate::state::AppState;
use zc_protocol::device::{DeviceInfo, DeviceStatus, FleetId, HardwareType};

/// Summary view of a device (for list responses).
#[derive(Debug, Serialize)]
pub struct DeviceSummary {
    pub device_id: String,
    pub status: DeviceStatus,
    pub hardware_type: HardwareType,
    pub last_heartbeat: Option<chrono::DateTime<chrono::Utc>>,
}

/// Request body for provisioning a new device.
#[derive(Debug, Deserialize)]
pub struct ProvisionDeviceRequest {
    pub device_id: String,
    pub fleet_id: String,
    pub hardware_type: String,
    pub vin: Option<String>,
    pub metadata: Option<serde_json::Value>,
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

/// POST /api/v1/devices — provision a new device.
pub async fn provision_device(
    State(state): State<AppState>,
    Json(req): Json<ProvisionDeviceRequest>,
) -> Result<(StatusCode, Json<DeviceInfo>), ApiError> {
    let now = Utc::now();
    let hw_type = parse_hardware_type(&req.hardware_type);
    let metadata = req.metadata.unwrap_or(serde_json::json!({}));
    // Merge fleet_id string into metadata for human-readable reference.
    let metadata = {
        let mut m = metadata;
        if let Some(obj) = m.as_object_mut() {
            obj.insert(
                "fleet".into(),
                serde_json::Value::String(req.fleet_id.clone()),
            );
        }
        m
    };

    if let Some(pool) = &state.pool {
        let exists = crate::db::devices::exists(pool, &req.device_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        if exists {
            return Err(ApiError::Conflict(format!(
                "device '{}' already exists",
                req.device_id
            )));
        }

        let row = crate::db::devices::DeviceRow {
            id: Uuid::now_v7(),
            fleet_id: Uuid::now_v7(),
            device_id: req.device_id.clone(),
            status: "provisioning".to_string(),
            vin: req.vin.clone(),
            hardware_type: req.hardware_type.clone(),
            certificate_id: None,
            last_heartbeat: None,
            metadata: metadata.clone(),
            created_at: now,
            updated_at: now,
        };
        crate::db::devices::insert(pool, &row)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        let device = row_to_device_info(row);

        let _ = state.event_tx.send(WsEvent::DeviceProvisioned {
            device_id: req.device_id,
            fleet_id: req.fleet_id,
            hardware_type: req.hardware_type,
            provisioned_at: now,
        });

        return Ok((StatusCode::CREATED, Json(device)));
    }

    // In-memory mode
    {
        let devices = state.devices.read().await;
        if devices.contains_key(&req.device_id) {
            return Err(ApiError::Conflict(format!(
                "device '{}' already exists",
                req.device_id
            )));
        }
    }

    let device = DeviceInfo {
        id: Uuid::now_v7(),
        fleet_id: FleetId(Uuid::now_v7()),
        device_id: req.device_id.clone(),
        status: DeviceStatus::Provisioning,
        vin: req.vin.clone(),
        hardware_type: hw_type,
        certificate_id: None,
        last_heartbeat: None,
        metadata,
        created_at: now,
        updated_at: now,
    };

    {
        let mut devices = state.devices.write().await;
        devices.insert(req.device_id.clone(), device.clone());
    }

    let _ = state.event_tx.send(WsEvent::DeviceProvisioned {
        device_id: req.device_id,
        fleet_id: req.fleet_id,
        hardware_type: req.hardware_type,
        provisioned_at: now,
    });

    Ok((StatusCode::CREATED, Json(device)))
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
        fleet_id: FleetId(r.fleet_id),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::build_router;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn app() -> axum::Router {
        build_router(AppState::with_sample_data())
    }

    #[tokio::test]
    async fn provision_valid_device() {
        let body = serde_json::json!({
            "device_id": "rpi-new-001",
            "fleet_id": "fleet-alpha",
            "hardware_type": "raspberry_pi_4",
            "vin": "1HGBH41JXMN109186"
        });

        let response = app()
            .oneshot(
                Request::post("/api/v1/devices")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["device_id"], "rpi-new-001");
        assert_eq!(json["status"], "provisioning");
        assert!(json["id"].is_string());
    }

    #[tokio::test]
    async fn provision_duplicate_device() {
        let body = serde_json::json!({
            "device_id": "rpi-001",
            "fleet_id": "fleet-alpha",
            "hardware_type": "raspberry_pi_4"
        });

        let response = app()
            .oneshot(
                Request::post("/api/v1/devices")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn provision_then_get() {
        let state = AppState::new();
        let app = build_router(state);

        let body = serde_json::json!({
            "device_id": "sbc-new-005",
            "fleet_id": "fleet-beta",
            "hardware_type": "industrial_sbc"
        });

        // Provision
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/v1/devices")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        // GET
        let response = app
            .oneshot(
                Request::get("/api/v1/devices/sbc-new-005")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["device_id"], "sbc-new-005");
    }

    #[tokio::test]
    async fn provision_broadcasts_event() {
        let state = AppState::new();
        let mut rx = state.event_tx.subscribe();
        let app = build_router(state);

        let body = serde_json::json!({
            "device_id": "rpi-event-001",
            "fleet_id": "fleet-alpha",
            "hardware_type": "raspberry_pi_5"
        });

        app.oneshot(
            Request::post("/api/v1/devices")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

        let event = rx.try_recv().unwrap();
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("device_provisioned"));
        assert!(json.contains("rpi-event-001"));
    }
}
