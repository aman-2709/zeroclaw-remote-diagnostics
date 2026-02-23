//! Shared test harness for E2E integration tests.
//!
//! Bridges the cloud API and fleet agent through a shared `MockChannel`,
//! exercising real code paths across all crate boundaries.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tokio::sync::broadcast;
use tower::ServiceExt;

use zc_canbus_tools::MockCanInterface;
use zc_cloud_api::events::WsEvent;
use zc_cloud_api::routes::build_router;
use zc_cloud_api::state::AppState;
use zc_fleet_agent::executor::CommandExecutor;
use zc_fleet_agent::registry::ToolRegistry;
use zc_log_tools::MockLogSource;
use zc_mqtt_channel::MockChannel;
use zc_protocol::commands::CommandEnvelope;

/// End-to-end test harness wiring cloud API + fleet agent via MockChannel.
pub struct TestHarness {
    /// Cloud API application state (in-memory, no DB).
    pub cloud_state: AppState,
    /// Axum router for HTTP requests via `tower::oneshot`.
    pub cloud_router: Router,
    /// Shared MQTT mock between cloud and agent.
    pub mqtt: Arc<MockChannel>,
    /// Fleet agent tool registry (9 tools: 5 CAN + 4 log).
    pub registry: ToolRegistry,
    /// Mock CAN bus interface for agent-side tool execution.
    pub can_interface: MockCanInterface,
    /// Mock log source for agent-side tool execution.
    pub log_source: MockLogSource,
    /// WebSocket event receiver for asserting broadcast events.
    pub event_rx: broadcast::Receiver<WsEvent>,
}

impl TestHarness {
    /// Create a new harness with sample devices (rpi-001, rpi-002, sbc-010).
    pub fn with_sample_data() -> Self {
        let mqtt = Arc::new(MockChannel::new());
        let mut state = AppState::with_sample_data();
        state.mqtt = Some(mqtt.clone());
        let event_rx = state.event_tx.subscribe();
        let router = build_router(state.clone());

        Self {
            cloud_state: state,
            cloud_router: router,
            mqtt,
            registry: ToolRegistry::with_defaults(),
            can_interface: MockCanInterface::new(),
            log_source: MockLogSource::with_syslog_sample(),
            event_rx,
        }
    }

    /// Create a harness with an empty device registry (no pre-populated devices).
    pub fn empty() -> Self {
        let mqtt = Arc::new(MockChannel::new());
        let mut state = AppState::new();
        state.mqtt = Some(mqtt.clone());
        let event_rx = state.event_tx.subscribe();
        let router = build_router(state.clone());

        Self {
            cloud_state: state,
            cloud_router: router,
            mqtt,
            registry: ToolRegistry::with_defaults(),
            can_interface: MockCanInterface::new(),
            log_source: MockLogSource::with_syslog_sample(),
            event_rx,
        }
    }

    /// Send a command via the cloud REST API (POST /api/v1/commands).
    /// Returns (HTTP status code, response JSON body).
    pub async fn send_command(
        &self,
        device_id: &str,
        fleet_id: &str,
        command: &str,
        initiated_by: &str,
    ) -> (StatusCode, serde_json::Value) {
        let body = serde_json::json!({
            "device_id": device_id,
            "fleet_id": fleet_id,
            "command": command,
            "initiated_by": initiated_by,
        });

        let response = self
            .cloud_router
            .clone()
            .oneshot(
                Request::post("/api/v1/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    /// Execute a command on the fleet agent side.
    /// Extracts the MQTT-published envelope and runs it through the CommandExecutor.
    pub async fn agent_execute(
        &self,
        envelope: &CommandEnvelope,
    ) -> zc_protocol::commands::CommandResponse {
        let executor = CommandExecutor::new(
            &self.registry,
            &self.can_interface,
            &self.log_source,
            None, // No Ollama for basic E2E — cloud provides parsed_intent
        );
        executor.execute(envelope).await
    }

    /// Ingest a command response on the cloud side via the MQTT bridge path.
    pub async fn cloud_ingest_response(&self, response: &zc_protocol::commands::CommandResponse) {
        let topic = zc_protocol::topics::command_response(&response.device_id, &response.device_id);
        let payload = serde_json::to_vec(response).unwrap();
        zc_cloud_api::mqtt_bridge::handle_incoming(&topic, &payload, &self.cloud_state).await;
    }

    /// Ingest a command response via the REST API (POST /api/v1/commands/{id}/respond).
    pub async fn rest_ingest_response(
        &self,
        response: &zc_protocol::commands::CommandResponse,
    ) -> (StatusCode, serde_json::Value) {
        let url = format!("/api/v1/commands/{}/respond", response.command_id);
        let resp = self
            .cloud_router
            .clone()
            .oneshot(
                Request::post(&url)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(response).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    /// Get a command record from in-memory cloud state.
    pub async fn get_command_record(
        &self,
        command_id: uuid::Uuid,
    ) -> Option<zc_cloud_api::state::CommandRecord> {
        let commands = self.cloud_state.commands.read().await;
        commands
            .iter()
            .find(|r| r.envelope.id == command_id)
            .cloned()
    }

    /// Provision a device via REST API (POST /api/v1/devices).
    pub async fn provision_device(
        &self,
        device_id: &str,
        fleet_id: &str,
        hardware_type: &str,
    ) -> (StatusCode, serde_json::Value) {
        let body = serde_json::json!({
            "device_id": device_id,
            "fleet_id": fleet_id,
            "hardware_type": hardware_type,
        });

        let response = self
            .cloud_router
            .clone()
            .oneshot(
                Request::post("/api/v1/devices")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    /// Send a heartbeat via REST API (POST /api/v1/heartbeat).
    pub async fn rest_heartbeat(
        &self,
        hb: &zc_protocol::device::Heartbeat,
    ) -> (StatusCode, serde_json::Value) {
        let response = self
            .cloud_router
            .clone()
            .oneshot(
                Request::post("/api/v1/heartbeat")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(hb).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    /// List shadows via REST API (GET /api/v1/devices/{id}/shadows).
    pub async fn list_shadows(&self, device_id: &str) -> (StatusCode, serde_json::Value) {
        let url = format!("/api/v1/devices/{device_id}/shadows");
        let response = self
            .cloud_router
            .clone()
            .oneshot(Request::get(&url).body(Body::empty()).unwrap())
            .await
            .unwrap();

        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    /// Get a shadow via REST API (GET /api/v1/devices/{id}/shadows/{name}).
    pub async fn get_shadow(
        &self,
        device_id: &str,
        shadow_name: &str,
    ) -> (StatusCode, serde_json::Value) {
        let url = format!("/api/v1/devices/{device_id}/shadows/{shadow_name}");
        let response = self
            .cloud_router
            .clone()
            .oneshot(Request::get(&url).body(Body::empty()).unwrap())
            .await
            .unwrap();

        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    /// Set desired shadow state via REST API (PUT /api/v1/devices/{id}/shadows/{name}/desired).
    pub async fn set_desired_shadow(
        &self,
        device_id: &str,
        shadow_name: &str,
        desired: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let url = format!("/api/v1/devices/{device_id}/shadows/{shadow_name}/desired");
        let body = serde_json::json!({"desired": desired});

        let response = self
            .cloud_router
            .clone()
            .oneshot(
                Request::put(&url)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    /// Ingest telemetry via REST API (POST /api/v1/devices/{id}/telemetry).
    pub async fn rest_ingest_telemetry(
        &self,
        device_id: &str,
        readings: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let url = format!("/api/v1/devices/{device_id}/telemetry");
        let body = serde_json::json!({ "readings": readings });

        let response = self
            .cloud_router
            .clone()
            .oneshot(
                Request::post(&url)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }
}
