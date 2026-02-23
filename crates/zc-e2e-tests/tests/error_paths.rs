//! E2E tests for error paths and edge cases across crate boundaries.

mod helpers;

use axum::http::StatusCode;
use chrono::Utc;
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use helpers::TestHarness;
use zc_protocol::commands::{
    ActionKind, CommandEnvelope, CommandResponse, CommandStatus, InferenceTier, ParsedIntent,
};

/// Sending a command to a device that doesn't exist returns 404.
#[tokio::test]
async fn e2e_command_to_unknown_device() {
    let h = TestHarness::with_sample_data();

    let (status, json) = h
        .send_command("ghost-999", "fleet-alpha", "read DTCs", "admin")
        .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(json["error"].as_str().unwrap().contains("ghost-999"));
}

/// A command with text that doesn't match any tool pattern — agent gets no parsed_intent
/// and without Ollama, fails gracefully.
#[tokio::test]
async fn e2e_unrecognized_command_no_intent() {
    let h = TestHarness::with_sample_data();

    let (status, cmd_json) = h
        .send_command("rpi-001", "fleet-alpha", "bake a pizza", "admin")
        .await;
    assert_eq!(status, StatusCode::OK);

    let envelope: CommandEnvelope = serde_json::from_slice(&h.mqtt.published()[0].payload).unwrap();

    // If RuleBasedEngine can't parse, parsed_intent is None.
    // Agent without Ollama should fail with descriptive error.
    let agent_resp = h.agent_execute(&envelope).await;

    // If intent is None and no Ollama, executor returns Failed.
    if envelope.parsed_intent.is_none() {
        assert_eq!(agent_resp.status, CommandStatus::Failed);
        assert!(agent_resp.error.is_some());
    }
    // Either way, the lifecycle completes without panic.
}

/// CAN tool timeout propagates as a failed command response.
#[tokio::test]
async fn e2e_can_timeout_propagates() {
    let h = TestHarness::with_sample_data();

    // Send a command that maps to a CAN tool (read_vin).
    // MockCanInterface has no queued responses → tool will fail/timeout.
    let mut envelope = CommandEnvelope::new("fleet-alpha", "rpi-001", "read VIN", "admin");
    envelope.parsed_intent = Some(ParsedIntent {
        action: ActionKind::Tool,
        tool_name: "read_vin".into(),
        tool_args: json!({}),
        confidence: 0.95,
    });

    let agent_resp = h.agent_execute(&envelope).await;

    // CAN tool with no mock data should fail
    assert_eq!(agent_resp.status, CommandStatus::Failed);
    assert!(agent_resp.error.is_some());
    assert_eq!(agent_resp.command_id, envelope.id);
}

/// Malformed MQTT response payload is silently dropped, no panic.
#[tokio::test]
async fn e2e_malformed_mqtt_response_ignored() {
    let h = TestHarness::with_sample_data();
    let mut rx = h.cloud_state.event_tx.subscribe();

    // Send garbage as a command response via MQTT bridge
    let topic = zc_protocol::topics::command_response("fleet-alpha", "rpi-001");
    zc_cloud_api::mqtt_bridge::handle_incoming(
        &topic,
        b"this is not valid json at all",
        &h.cloud_state,
    )
    .await;

    // No event should be broadcast for malformed data
    assert!(
        rx.try_recv().is_err(),
        "malformed payload should not produce event"
    );
}

/// Response for a command_id that doesn't exist in cloud state is handled gracefully.
#[tokio::test]
async fn e2e_response_for_unknown_command() {
    let h = TestHarness::with_sample_data();

    let fake_id = Uuid::now_v7();
    let resp = CommandResponse {
        command_id: fake_id,
        correlation_id: fake_id,
        device_id: "rpi-001".into(),
        status: CommandStatus::Completed,
        inference_tier: InferenceTier::Local,
        response_text: Some("phantom response".into()),
        response_data: None,
        latency_ms: 10,
        responded_at: Utc::now(),
        error: None,
    };

    // REST path: should return 404
    let (status, _) = h.rest_ingest_response(&resp).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Response with mismatched command_id in path vs body returns 400.
#[tokio::test]
async fn e2e_response_id_mismatch() {
    let h = TestHarness::with_sample_data();

    // First create a command so we have a valid path ID
    let (_, cmd_json) = h
        .send_command("rpi-001", "fleet-alpha", "read DTCs", "admin")
        .await;
    let cmd_id: Uuid = cmd_json["id"].as_str().unwrap().parse().unwrap();

    // But send a response with a DIFFERENT command_id in the body
    let wrong_id = Uuid::now_v7();
    let resp = CommandResponse {
        command_id: wrong_id, // doesn't match path
        correlation_id: wrong_id,
        device_id: "rpi-001".into(),
        status: CommandStatus::Completed,
        inference_tier: InferenceTier::Local,
        response_text: None,
        response_data: None,
        latency_ms: 10,
        responded_at: Utc::now(),
        error: None,
    };

    // POST to the correct command path, but body has wrong ID
    let url = format!("/api/v1/commands/{cmd_id}/respond");
    let http_resp = h
        .cloud_router
        .clone()
        .oneshot(
            axum::http::Request::post(&url)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(serde_json::to_vec(&resp).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(http_resp.status(), StatusCode::BAD_REQUEST);
}

/// Sending a command with a parsed_intent pointing to an unknown tool.
#[tokio::test]
async fn e2e_unknown_tool_in_intent() {
    let h = TestHarness::with_sample_data();

    let mut envelope = CommandEnvelope::new("fleet-alpha", "rpi-001", "do magic", "admin");
    envelope.parsed_intent = Some(ParsedIntent {
        action: ActionKind::Tool,
        tool_name: "self_destruct".into(),
        tool_args: json!({}),
        confidence: 0.99,
    });

    let agent_resp = h.agent_execute(&envelope).await;
    assert_eq!(agent_resp.status, CommandStatus::Failed);
    assert!(agent_resp.error.as_ref().unwrap().contains("unknown tool"));
}

/// Empty command text goes through the lifecycle without panic.
#[tokio::test]
async fn e2e_empty_command_text() {
    let h = TestHarness::with_sample_data();

    let (status, cmd_json) = h.send_command("rpi-001", "fleet-alpha", "", "admin").await;
    assert_eq!(status, StatusCode::OK);

    // The envelope is still created and published
    let published = h.mqtt.published();
    assert!(!published.is_empty());

    let envelope: CommandEnvelope = serde_json::from_slice(&published[0].payload).unwrap();
    assert_eq!(envelope.natural_language, "");

    // Agent execution with empty text: either fails gracefully or succeeds
    // depending on inference/intent state — no panic is the key assertion.
    let _agent_resp = h.agent_execute(&envelope).await;
}
