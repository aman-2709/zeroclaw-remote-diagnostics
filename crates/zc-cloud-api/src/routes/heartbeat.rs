//! Heartbeat ingestion endpoint.

use axum::Json;
use axum::extract::State;
use chrono::Utc;

use crate::error::{ApiError, ApiResult};
use crate::events::WsEvent;
use crate::state::AppState;
use zc_protocol::device::Heartbeat;

/// POST /api/v1/heartbeat — ingest a device heartbeat.
pub async fn ingest_heartbeat(
    State(state): State<AppState>,
    Json(hb): Json<Heartbeat>,
) -> ApiResult<Json<serde_json::Value>> {
    // Update last_heartbeat in the database
    if let Some(pool) = &state.pool {
        crate::db::devices::update_heartbeat(pool, &hb.device_id, hb.timestamp)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
    } else {
        // In-memory: update device heartbeat timestamp
        let mut devices = state.devices.write().await;
        if let Some(device) = devices.get_mut(&hb.device_id) {
            device.last_heartbeat = Some(hb.timestamp);
        }
    }

    tracing::debug!(device_id = %hb.device_id, "heartbeat received");

    // Broadcast real-time event
    let _ = state.event_tx.send(WsEvent::DeviceHeartbeat {
        device_id: hb.device_id.clone(),
        timestamp: Utc::now(),
    });

    Ok(Json(serde_json::json!({ "status": "ok" })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::build_router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use zc_protocol::device::ServiceStatus;

    fn app() -> axum::Router {
        build_router(AppState::with_sample_data())
    }

    #[tokio::test]
    async fn heartbeat_updates_device() {
        let heartbeat = Heartbeat {
            device_id: "rpi-001".into(),
            fleet_id: "fleet-alpha".into(),
            status: zc_protocol::device::DeviceStatus::Online,
            uptime_secs: 3600,
            ollama_status: ServiceStatus::Running,
            can_status: ServiceStatus::Running,
            agent_version: "0.1.0".into(),
            timestamp: Utc::now(),
        };

        let response = app()
            .oneshot(
                Request::post("/api/v1/heartbeat")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&heartbeat).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn heartbeat_broadcasts_event() {
        let state = AppState::with_sample_data();
        let mut rx = state.event_tx.subscribe();
        let app = build_router(state);

        let heartbeat = Heartbeat {
            device_id: "rpi-001".into(),
            fleet_id: "fleet-alpha".into(),
            status: zc_protocol::device::DeviceStatus::Online,
            uptime_secs: 7200,
            ollama_status: ServiceStatus::Running,
            can_status: ServiceStatus::Stopped,
            agent_version: "0.1.0".into(),
            timestamp: Utc::now(),
        };

        app.oneshot(
            Request::post("/api/v1/heartbeat")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&heartbeat).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

        let event = rx.try_recv().unwrap();
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("device_heartbeat"));
        assert!(json.contains("rpi-001"));
    }
}
