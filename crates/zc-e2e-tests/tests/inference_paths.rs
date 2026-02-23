//! E2E tests for inference engine integration across cloud API and fleet agent.

mod helpers;

use axum::http::StatusCode;
use serde_json::json;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use helpers::TestHarness;
use zc_fleet_agent::executor::CommandExecutor;
use zc_fleet_agent::inference::{OllamaClient, OllamaConfig};
use zc_protocol::commands::{ActionKind, CommandEnvelope, CommandStatus, ParsedIntent};

/// All 9 tools are parseable through the RuleBasedEngine via the REST API.
#[tokio::test]
async fn e2e_all_nine_tools_parseable() {
    let h = TestHarness::with_sample_data();

    // Map of command text → expected tool_name for RuleBasedEngine patterns
    let tool_commands = [
        ("read DTCs", "read_dtcs"),
        ("read VIN number", "read_vin"),
        ("read freeze frame", "read_freeze"),
        ("read engine RPM", "read_pid"),
        ("monitor CAN bus", "can_monitor"),
        ("search logs for error", "search_logs"),
        ("analyze errors in logs", "analyze_errors"),
        ("show log stats", "log_stats"),
        ("tail logs", "tail_logs"),
    ];

    for (command_text, expected_tool) in &tool_commands {
        let (status, cmd_json) = h
            .send_command("rpi-001", "fleet-alpha", command_text, "admin")
            .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "command '{command_text}' should dispatch successfully"
        );

        // Verify the envelope was published and has the expected tool
        let published = h.mqtt.published();
        let last = published.last().unwrap();
        let envelope: CommandEnvelope = serde_json::from_slice(&last.payload).unwrap();

        assert!(
            envelope.parsed_intent.is_some(),
            "RuleBasedEngine should parse '{command_text}' into a tool"
        );
        assert_eq!(
            envelope.parsed_intent.as_ref().unwrap().tool_name,
            *expected_tool,
            "'{command_text}' should map to '{expected_tool}'"
        );
    }
}

/// Rule-based inference drives agent execution end-to-end.
#[tokio::test]
async fn e2e_rule_based_drives_execution() {
    let h = TestHarness::with_sample_data();

    let (_, cmd_json) = h
        .send_command("rpi-001", "fleet-alpha", "show log stats", "admin")
        .await;
    let cmd_id: Uuid = cmd_json["id"].as_str().unwrap().parse().unwrap();

    // Verify cloud inference set the intent
    let envelope: CommandEnvelope = serde_json::from_slice(&h.mqtt.published()[0].payload).unwrap();
    assert_eq!(
        envelope.parsed_intent.as_ref().unwrap().tool_name,
        "log_stats"
    );

    // Agent uses the pre-parsed intent
    let agent_resp = h.agent_execute(&envelope).await;
    assert_eq!(agent_resp.command_id, cmd_id);
    assert_eq!(agent_resp.status, CommandStatus::Completed);
    assert!(agent_resp.response_data.is_some());
}

/// When parsed_intent is pre-set on the envelope, agent uses it directly (fast path).
#[tokio::test]
async fn e2e_pre_parsed_intent_used() {
    let h = TestHarness::with_sample_data();

    let mut envelope = CommandEnvelope::new(
        "fleet-alpha",
        "rpi-001",
        "custom command text that won't match rules",
        "admin",
    );
    envelope.parsed_intent = Some(ParsedIntent {
        action: ActionKind::Tool,
        tool_name: "log_stats".into(),
        tool_args: json!({"path": "/var/log/syslog"}),
        confidence: 0.95,
    });

    let agent_resp = h.agent_execute(&envelope).await;
    assert_eq!(agent_resp.status, CommandStatus::Completed);
    assert!(agent_resp.response_data.is_some());
    assert!(
        agent_resp
            .response_text
            .as_ref()
            .unwrap()
            .contains("log_stats")
    );
}

/// Ollama fallback on agent side: wiremock simulates Ollama returning a valid tool match.
#[tokio::test]
async fn e2e_ollama_fallback_on_agent() {
    let h = TestHarness::with_sample_data();

    // Start wiremock Ollama server
    let server = MockServer::start().await;
    let ollama_response = json!({
        "model": "phi3:mini",
        "message": {
            "role": "assistant",
            "content": r#"{"tool_name": "log_stats", "tool_args": {"path": "/var/log/syslog"}, "confidence": 0.92}"#
        },
        "done": true
    });
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&ollama_response))
        .mount(&server)
        .await;

    let ollama = OllamaClient::new(OllamaConfig {
        host: server.uri(),
        model: "phi3:mini".into(),
        timeout_secs: 2,
        enabled: true,
    });

    // Envelope WITHOUT parsed_intent — forces Ollama inference
    let envelope = CommandEnvelope::new(
        "fleet-alpha",
        "rpi-001",
        "show me the log statistics",
        "admin",
    );

    let executor =
        CommandExecutor::new(&h.registry, &h.can_interface, &h.log_source, Some(&ollama));
    let agent_resp = executor.execute(&envelope).await;

    assert_eq!(agent_resp.status, CommandStatus::Completed);
    assert!(agent_resp.response_data.is_some());
}

/// Inference tier information propagates through the command chain.
#[tokio::test]
async fn e2e_inference_tier_tracked() {
    let h = TestHarness::with_sample_data();

    let (_, cmd_json) = h
        .send_command("rpi-001", "fleet-alpha", "read DTCs", "admin")
        .await;
    let cmd_id: Uuid = cmd_json["id"].as_str().unwrap().parse().unwrap();

    let envelope: CommandEnvelope = serde_json::from_slice(&h.mqtt.published()[0].payload).unwrap();

    // Cloud inference should have set a tier
    assert!(
        envelope.parsed_intent.is_some(),
        "rule-based engine should parse 'read DTCs'"
    );

    // Agent response carries the inference tier
    let agent_resp = h.agent_execute(&envelope).await;
    // InferenceTier is set (Local for pre-parsed intent path)
    assert_eq!(
        agent_resp.inference_tier,
        zc_protocol::commands::InferenceTier::Local
    );
}
