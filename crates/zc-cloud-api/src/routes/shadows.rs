//! Shadow REST endpoints for querying and setting device shadow state.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use rumqttc::QoS;
use serde::{Deserialize, Serialize};

use zc_protocol::shadows::{ShadowDelta, ShadowState};
use zc_protocol::topics;

use crate::events::WsEvent;
use crate::mqtt_bridge::compute_delta;
use crate::state::AppState;

/// Summary of a named shadow.
#[derive(Debug, Serialize)]
pub struct ShadowSummary {
    pub shadow_name: String,
    pub version: u64,
    pub last_updated: String,
}

/// Full shadow response including computed delta.
#[derive(Debug, Serialize)]
pub struct ShadowResponse {
    pub device_id: String,
    pub shadow_name: String,
    pub reported: serde_json::Value,
    pub desired: serde_json::Value,
    pub delta: serde_json::Value,
    pub version: u64,
    pub last_updated: String,
}

/// Request body for setting desired state.
#[derive(Debug, Deserialize)]
pub struct SetDesiredRequest {
    pub desired: serde_json::Value,
}

/// GET /api/v1/devices/{id}/shadows — list all shadows for a device.
pub async fn list_shadows(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
) -> Result<Json<Vec<ShadowSummary>>, StatusCode> {
    if let Some(pool) = &state.pool {
        let rows = crate::db::shadows::list_shadows(pool, &device_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let summaries: Vec<ShadowSummary> = rows
            .into_iter()
            .map(|r| ShadowSummary {
                shadow_name: r.shadow_name,
                version: r.version as u64,
                last_updated: r.last_updated.to_rfc3339(),
            })
            .collect();
        Ok(Json(summaries))
    } else {
        let shadows = state.shadows.read().await;
        let summaries: Vec<ShadowSummary> = shadows
            .iter()
            .filter(|((did, _), _)| did == &device_id)
            .map(|((_, name), s)| ShadowSummary {
                shadow_name: name.clone(),
                version: s.version,
                last_updated: s.last_updated.to_rfc3339(),
            })
            .collect();
        Ok(Json(summaries))
    }
}

/// GET /api/v1/devices/{id}/shadows/{name} — get a specific shadow.
pub async fn get_shadow(
    State(state): State<AppState>,
    Path((device_id, shadow_name)): Path<(String, String)>,
) -> Result<Json<ShadowResponse>, StatusCode> {
    if let Some(pool) = &state.pool {
        let row = crate::db::shadows::get_shadow(pool, &device_id, &shadow_name)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::NOT_FOUND)?;
        let delta = compute_delta(&row.desired, &row.reported);
        Ok(Json(ShadowResponse {
            device_id: row.device_id,
            shadow_name: row.shadow_name,
            reported: row.reported,
            desired: row.desired,
            delta,
            version: row.version as u64,
            last_updated: row.last_updated.to_rfc3339(),
        }))
    } else {
        let shadows = state.shadows.read().await;
        let key = (device_id.clone(), shadow_name.clone());
        let shadow = shadows.get(&key).ok_or(StatusCode::NOT_FOUND)?;
        let delta = compute_delta(&shadow.desired, &shadow.reported);
        Ok(Json(ShadowResponse {
            device_id,
            shadow_name,
            reported: shadow.reported.clone(),
            desired: shadow.desired.clone(),
            delta,
            version: shadow.version,
            last_updated: shadow.last_updated.to_rfc3339(),
        }))
    }
}

/// PUT /api/v1/devices/{id}/shadows/{name}/desired — set desired state.
pub async fn set_desired(
    State(state): State<AppState>,
    Path((device_id, shadow_name)): Path<(String, String)>,
    Json(req): Json<SetDesiredRequest>,
) -> Result<Json<ShadowResponse>, StatusCode> {
    let reported;
    let version;
    let last_updated;

    if let Some(pool) = &state.pool {
        let row = crate::db::shadows::set_desired(pool, &device_id, &shadow_name, &req.desired)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        reported = row.reported;
        version = row.version as u64;
        last_updated = row.last_updated;
    } else {
        let mut shadows = state.shadows.write().await;
        let key = (device_id.clone(), shadow_name.clone());
        let entry = shadows.entry(key).or_insert_with(|| ShadowState {
            reported: serde_json::Value::Object(Default::default()),
            desired: serde_json::Value::Object(Default::default()),
            version: 0,
            last_updated: Utc::now(),
        });
        entry.desired = req.desired.clone();
        entry.version += 1;
        entry.last_updated = Utc::now();
        reported = entry.reported.clone();
        version = entry.version;
        last_updated = entry.last_updated;
    }

    let delta = compute_delta(&req.desired, &reported);

    // Publish ShadowDelta via MQTT if there's a difference.
    if !delta.as_object().is_none_or(|o| o.is_empty())
        && let Some(mqtt) = &state.mqtt
    {
        // Derive fleet_id from device info or use a default.
        let fleet_id = if state.pool.is_some() {
            // In DB mode, we'd look it up — for now just use the device_id path.
            // The topic is fleet-level, but cloud bridge knows the fleet context.
            String::new()
        } else {
            let devices = state.devices.read().await;
            devices
                .get(&device_id)
                .and_then(|d| d.metadata.get("fleet"))
                .and_then(|v| v.as_str())
                .unwrap_or("default")
                .to_string()
        };

        let shadow_delta = ShadowDelta {
            device_id: device_id.clone(),
            shadow_name: shadow_name.clone(),
            delta: delta.clone(),
            version,
            timestamp: Utc::now(),
        };

        let topic = topics::shadow_delta(&fleet_id, &device_id);
        if let Ok(payload) = serde_json::to_vec(&shadow_delta) {
            let _ = mqtt.publish(&topic, &payload, QoS::AtLeastOnce).await;
        }
    }

    // Broadcast event.
    let _ = state.event_tx.send(WsEvent::ShadowUpdated {
        device_id: device_id.clone(),
        shadow_name: shadow_name.clone(),
        version,
        timestamp: Utc::now(),
    });

    Ok(Json(ShadowResponse {
        device_id,
        shadow_name,
        reported,
        desired: req.desired,
        delta,
        version,
        last_updated: last_updated.to_rfc3339(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn app() -> axum::Router {
        crate::routes::build_router(AppState::with_sample_data())
    }

    fn app_with_state(state: AppState) -> axum::Router {
        crate::routes::build_router(state)
    }

    #[tokio::test]
    async fn list_shadows_empty() {
        let response = app()
            .oneshot(
                Request::get("/api/v1/devices/rpi-001/shadows")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(json.is_empty());
    }

    #[tokio::test]
    async fn get_shadow_not_found() {
        let response = app()
            .oneshot(
                Request::get("/api/v1/devices/rpi-001/shadows/diagnostics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn set_desired_creates_shadow() {
        let state = AppState::with_sample_data();
        let router = app_with_state(state.clone());

        let body = serde_json::json!({"desired": {"firmware": "0.2.0"}});
        let response = router
            .oneshot(
                Request::put("/api/v1/devices/rpi-001/shadows/config/desired")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["shadow_name"], "config");
        assert_eq!(json["desired"]["firmware"], "0.2.0");
        assert_eq!(json["version"], 1);
    }

    #[tokio::test]
    async fn get_shadow_after_set() {
        let state = AppState::with_sample_data();

        // Set desired first.
        {
            let mut shadows = state.shadows.write().await;
            shadows.insert(
                ("rpi-001".to_string(), "diag".to_string()),
                ShadowState {
                    reported: serde_json::json!({"firmware": "0.1.0"}),
                    desired: serde_json::json!({"firmware": "0.2.0"}),
                    version: 3,
                    last_updated: Utc::now(),
                },
            );
        }

        let router = app_with_state(state);
        let response = router
            .oneshot(
                Request::get("/api/v1/devices/rpi-001/shadows/diag")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["reported"]["firmware"], "0.1.0");
        assert_eq!(json["desired"]["firmware"], "0.2.0");
        assert_eq!(json["delta"]["firmware"], "0.2.0");
    }

    #[tokio::test]
    async fn set_desired_publishes_delta() {
        let mqtt = std::sync::Arc::new(zc_mqtt_channel::MockChannel::new());
        let mut state = AppState::with_sample_data();
        state.mqtt = Some(mqtt.clone());

        // Pre-populate reported state.
        {
            let mut shadows = state.shadows.write().await;
            shadows.insert(
                ("rpi-001".to_string(), "config".to_string()),
                ShadowState {
                    reported: serde_json::json!({"firmware": "0.1.0"}),
                    desired: serde_json::json!({}),
                    version: 1,
                    last_updated: Utc::now(),
                },
            );
        }

        let router = app_with_state(state);
        let body = serde_json::json!({"desired": {"firmware": "0.2.0"}});
        let _response = router
            .oneshot(
                Request::put("/api/v1/devices/rpi-001/shadows/config/desired")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Verify delta was published via MQTT.
        let delta_msgs = mqtt.published_to("fleet/fleet-alpha/rpi-001/shadow/delta");
        assert_eq!(delta_msgs.len(), 1);
        let delta: ShadowDelta = serde_json::from_slice(&delta_msgs[0].payload).unwrap();
        assert_eq!(delta.delta["firmware"], "0.2.0");
    }
}
