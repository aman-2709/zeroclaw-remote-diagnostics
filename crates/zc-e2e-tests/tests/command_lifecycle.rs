//! E2E tests for the full command lifecycle:
//! REST API → inference → MQTT publish → agent execution → response ingestion → WebSocket event.

mod helpers;

use axum::http::StatusCode;
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use helpers::TestHarness;
use zc_protocol::commands::{CommandStatus, InferenceTier, ParsedIntent};

/// Full lifecycle: send "search logs" → cloud inference → agent executes → response ingested.
#[tokio::test]
async fn e2e_search_logs_full_lifecycle() {
    let h = TestHarness::with_sample_data();

    // 1. Send command via REST API
    let (status, cmd_json) = h
        .send_command("rpi-001", "fleet-alpha", "search logs for error", "admin")
        .await;
    assert_eq!(status, StatusCode::OK);
    let cmd_id: Uuid = cmd_json["id"].as_str().unwrap().parse().unwrap();

    // 2. Verify command was published to MQTT
    let published = h.mqtt.published();
    assert!(!published.is_empty(), "command should be published to MQTT");

    // 3. Deserialize the MQTT-published envelope
    let envelope: zc_protocol::commands::CommandEnvelope =
        serde_json::from_slice(&published[0].payload).unwrap();
    assert_eq!(envelope.id, cmd_id);

    // 4. Agent executes the command (cloud already parsed intent via RuleBasedEngine)
    let agent_resp = h.agent_execute(&envelope).await;
    assert_eq!(agent_resp.command_id, cmd_id);
    assert_eq!(agent_resp.status, CommandStatus::Completed);

    // 5. Ingest response back into cloud via REST
    let (resp_status, _) = h.rest_ingest_response(&agent_resp).await;
    assert_eq!(resp_status, StatusCode::OK);

    // 6. Verify command record updated
    let record = h.get_command_record(cmd_id).await.unwrap();
    assert!(record.response.is_some());
    assert_eq!(record.response.unwrap().status, CommandStatus::Completed);
}

/// Full lifecycle for CAN bus tool: "read DTCs".
#[tokio::test]
async fn e2e_read_dtcs_full_lifecycle() {
    let h = TestHarness::with_sample_data();

    let (status, cmd_json) = h
        .send_command("rpi-001", "fleet-alpha", "read DTCs", "admin")
        .await;
    assert_eq!(status, StatusCode::OK);
    let cmd_id: Uuid = cmd_json["id"].as_str().unwrap().parse().unwrap();

    let envelope: zc_protocol::commands::CommandEnvelope =
        serde_json::from_slice(&h.mqtt.published()[0].payload).unwrap();

    let agent_resp = h.agent_execute(&envelope).await;
    assert_eq!(agent_resp.command_id, cmd_id);
    // CAN tool with mock interface — may complete or fail depending on mock state,
    // but the lifecycle should complete without panic.
    assert!(
        agent_resp.status == CommandStatus::Completed || agent_resp.status == CommandStatus::Failed
    );

    let (resp_status, _) = h.rest_ingest_response(&agent_resp).await;
    assert_eq!(resp_status, StatusCode::OK);
}

/// Full lifecycle for read_pid with a pre-parsed intent (bypassing cloud inference).
#[tokio::test]
async fn e2e_read_pid_with_parsed_intent() {
    let h = TestHarness::with_sample_data();

    let (status, cmd_json) = h
        .send_command("rpi-001", "fleet-alpha", "read engine RPM", "admin")
        .await;
    assert_eq!(status, StatusCode::OK);
    let cmd_id: Uuid = cmd_json["id"].as_str().unwrap().parse().unwrap();

    // Extract and verify MQTT envelope has parsed_intent from RuleBasedEngine
    let envelope: zc_protocol::commands::CommandEnvelope =
        serde_json::from_slice(&h.mqtt.published()[0].payload).unwrap();
    assert!(
        envelope.parsed_intent.is_some(),
        "RuleBasedEngine should parse 'read engine RPM' into read_pid"
    );
    assert_eq!(
        envelope.parsed_intent.as_ref().unwrap().tool_name,
        "read_pid"
    );

    let agent_resp = h.agent_execute(&envelope).await;
    assert_eq!(agent_resp.command_id, cmd_id);
}

/// Command ID, correlation_id, and device_id are preserved across the entire lifecycle.
#[tokio::test]
async fn e2e_ids_preserved_across_lifecycle() {
    let h = TestHarness::with_sample_data();

    let (_, cmd_json) = h
        .send_command("rpi-002", "fleet-alpha", "show log stats", "operator")
        .await;
    let cmd_id: Uuid = cmd_json["id"].as_str().unwrap().parse().unwrap();
    let correlation_id: Uuid = cmd_json["correlation_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    // MQTT envelope preserves IDs
    let envelope: zc_protocol::commands::CommandEnvelope =
        serde_json::from_slice(&h.mqtt.published()[0].payload).unwrap();
    assert_eq!(envelope.id, cmd_id);
    assert_eq!(envelope.correlation_id, correlation_id);
    assert_eq!(envelope.device_id, "rpi-002");

    // Agent response preserves IDs
    let agent_resp = h.agent_execute(&envelope).await;
    assert_eq!(agent_resp.command_id, cmd_id);
    assert_eq!(agent_resp.correlation_id, correlation_id);
    assert_eq!(agent_resp.device_id, "rpi-002");
}

/// Both CommandDispatched and CommandResponse WebSocket events fire during lifecycle.
#[tokio::test]
async fn e2e_websocket_events_dispatched_and_received() {
    let h = TestHarness::with_sample_data();
    let mut rx = h.cloud_state.event_tx.subscribe();

    // Send command → triggers CommandDispatched event
    let (_, cmd_json) = h
        .send_command("rpi-001", "fleet-alpha", "tail logs", "admin")
        .await;
    let cmd_id: Uuid = cmd_json["id"].as_str().unwrap().parse().unwrap();

    let dispatched = rx.try_recv().unwrap();
    let dispatched_json = serde_json::to_string(&dispatched).unwrap();
    assert!(dispatched_json.contains("command_dispatched"));

    // Execute on agent and ingest response → triggers CommandResponse event
    let envelope: zc_protocol::commands::CommandEnvelope =
        serde_json::from_slice(&h.mqtt.published()[0].payload).unwrap();
    let agent_resp = h.agent_execute(&envelope).await;
    h.rest_ingest_response(&agent_resp).await;

    let response_event = rx.try_recv().unwrap();
    let response_json = serde_json::to_string(&response_event).unwrap();
    assert!(response_json.contains("command_response"));
    assert!(response_json.contains(&cmd_id.to_string()));
}

/// Command status transitions from pending (no response) to completed after response.
#[tokio::test]
async fn e2e_command_status_transitions() {
    let h = TestHarness::with_sample_data();

    let (_, cmd_json) = h
        .send_command("rpi-001", "fleet-alpha", "analyze errors", "admin")
        .await;
    let cmd_id: Uuid = cmd_json["id"].as_str().unwrap().parse().unwrap();

    // Before response: no response attached
    let record = h.get_command_record(cmd_id).await.unwrap();
    assert!(record.response.is_none());

    // Execute and ingest response
    let envelope: zc_protocol::commands::CommandEnvelope =
        serde_json::from_slice(&h.mqtt.published()[0].payload).unwrap();
    let agent_resp = h.agent_execute(&envelope).await;
    h.rest_ingest_response(&agent_resp).await;

    // After response: response attached with status
    let record = h.get_command_record(cmd_id).await.unwrap();
    assert!(record.response.is_some());
}

/// Latency_ms is populated on the agent response and visible in ingested record.
#[tokio::test]
async fn e2e_latency_computed_on_response() {
    let h = TestHarness::with_sample_data();

    let (_, cmd_json) = h
        .send_command("rpi-001", "fleet-alpha", "log stats", "admin")
        .await;
    let cmd_id: Uuid = cmd_json["id"].as_str().unwrap().parse().unwrap();

    let envelope: zc_protocol::commands::CommandEnvelope =
        serde_json::from_slice(&h.mqtt.published()[0].payload).unwrap();
    let agent_resp = h.agent_execute(&envelope).await;

    // Agent response should have a latency measurement
    assert!(agent_resp.latency_ms < 5000, "latency should be reasonable");

    h.rest_ingest_response(&agent_resp).await;

    let record = h.get_command_record(cmd_id).await.unwrap();
    let resp = record.response.unwrap();
    assert!(resp.latency_ms < 5000);
}
