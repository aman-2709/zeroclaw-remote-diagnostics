//! E2E tests for concurrent and multi-device command scenarios.

mod helpers;

use axum::http::StatusCode;
use uuid::Uuid;

use helpers::TestHarness;
use zc_protocol::commands::{CommandEnvelope, CommandStatus};

/// Three devices receive commands simultaneously; all complete independently.
#[tokio::test]
async fn e2e_concurrent_commands_multiple_devices() {
    let h = TestHarness::with_sample_data();

    let devices = ["rpi-001", "rpi-002", "sbc-010"];
    let mut cmd_ids = Vec::new();

    // Dispatch commands to all three devices
    for device in &devices {
        let (status, cmd_json) = h
            .send_command(device, "fleet-alpha", "show log stats", "admin")
            .await;
        assert_eq!(status, StatusCode::OK);
        let cmd_id: Uuid = cmd_json["id"].as_str().unwrap().parse().unwrap();
        cmd_ids.push(cmd_id);
    }

    // Verify all three were published to MQTT
    let published = h.mqtt.published();
    assert_eq!(published.len(), 3);

    // Execute each on the agent side and ingest responses
    for msg in &published {
        let envelope: CommandEnvelope = serde_json::from_slice(&msg.payload).unwrap();
        let agent_resp = h.agent_execute(&envelope).await;
        assert_eq!(agent_resp.status, CommandStatus::Completed);

        let (resp_status, _) = h.rest_ingest_response(&agent_resp).await;
        assert_eq!(resp_status, StatusCode::OK);
    }

    // Verify all three commands have responses
    for cmd_id in &cmd_ids {
        let record = h.get_command_record(*cmd_id).await.unwrap();
        assert!(record.response.is_some());
    }
}

/// 10 sequential commands to the same device, all complete in order.
#[tokio::test]
async fn e2e_sequential_commands_same_device() {
    let h = TestHarness::with_sample_data();

    let commands = [
        "read DTCs",
        "show log stats",
        "analyze errors",
        "tail logs",
        "search logs for warning",
        "read DTCs",
        "log stats",
        "search logs for critical",
        "tail logs",
        "analyze errors in logs",
    ];

    let mut cmd_ids = Vec::new();
    for cmd in &commands {
        let (status, cmd_json) = h.send_command("rpi-001", "fleet-alpha", cmd, "admin").await;
        assert_eq!(status, StatusCode::OK, "command '{cmd}' should dispatch");
        let cmd_id: Uuid = cmd_json["id"].as_str().unwrap().parse().unwrap();
        cmd_ids.push(cmd_id);
    }

    assert_eq!(h.mqtt.published().len(), 10);

    // Execute all and ingest responses
    for msg in &h.mqtt.published() {
        let envelope: CommandEnvelope = serde_json::from_slice(&msg.payload).unwrap();
        let agent_resp = h.agent_execute(&envelope).await;

        let (resp_status, _) = h.rest_ingest_response(&agent_resp).await;
        assert_eq!(resp_status, StatusCode::OK);
    }

    // All 10 commands should have responses
    for cmd_id in &cmd_ids {
        let record = h.get_command_record(*cmd_id).await.unwrap();
        assert!(
            record.response.is_some(),
            "command {cmd_id} should have response"
        );
    }
}

/// Commands to different fleets are independent — fleet-alpha and fleet-beta.
#[tokio::test]
async fn e2e_cross_fleet_isolation() {
    let h = TestHarness::with_sample_data();
    // Sample data has rpi-001 (fleet-alpha), rpi-002 (fleet-alpha), sbc-010 (fleet-beta)

    let (status_alpha, _) = h
        .send_command("rpi-001", "fleet-alpha", "read DTCs", "admin-alpha")
        .await;
    assert_eq!(status_alpha, StatusCode::OK);

    let (status_beta, _) = h
        .send_command("sbc-010", "fleet-beta", "show log stats", "admin-beta")
        .await;
    assert_eq!(status_beta, StatusCode::OK);

    // Both published to MQTT with correct fleet_ids
    let published = h.mqtt.published();
    assert_eq!(published.len(), 2);

    let env_alpha: CommandEnvelope = serde_json::from_slice(&published[0].payload).unwrap();
    let env_beta: CommandEnvelope = serde_json::from_slice(&published[1].payload).unwrap();

    assert_eq!(env_alpha.fleet_id, "fleet-alpha");
    assert_eq!(env_alpha.device_id, "rpi-001");
    assert_eq!(env_beta.fleet_id, "fleet-beta");
    assert_eq!(env_beta.device_id, "sbc-010");
}

/// Two overlapping (in-flight) commands to the same device resolve independently.
#[tokio::test]
async fn e2e_overlapping_commands_independent() {
    let h = TestHarness::with_sample_data();

    // Dispatch two commands without waiting for responses
    let (_, cmd1_json) = h
        .send_command("rpi-001", "fleet-alpha", "read DTCs", "admin")
        .await;
    let (_, cmd2_json) = h
        .send_command("rpi-001", "fleet-alpha", "show log stats", "admin")
        .await;

    let cmd1_id: Uuid = cmd1_json["id"].as_str().unwrap().parse().unwrap();
    let cmd2_id: Uuid = cmd2_json["id"].as_str().unwrap().parse().unwrap();
    assert_ne!(cmd1_id, cmd2_id, "two commands should have different IDs");

    // Execute both on agent side
    let published = h.mqtt.published();
    assert_eq!(published.len(), 2);

    // Execute in reverse order to test independence
    let env2: CommandEnvelope = serde_json::from_slice(&published[1].payload).unwrap();
    let resp2 = h.agent_execute(&env2).await;
    h.rest_ingest_response(&resp2).await;

    let env1: CommandEnvelope = serde_json::from_slice(&published[0].payload).unwrap();
    let resp1 = h.agent_execute(&env1).await;
    h.rest_ingest_response(&resp1).await;

    // Both commands should have responses
    let record1 = h.get_command_record(cmd1_id).await.unwrap();
    let record2 = h.get_command_record(cmd2_id).await.unwrap();
    assert!(record1.response.is_some());
    assert!(record2.response.is_some());
}

/// Provision a new device, then send a command to it.
#[tokio::test]
async fn e2e_provision_then_command() {
    let h = TestHarness::with_sample_data();

    // Provision new device
    let (prov_status, prov_json) = h
        .provision_device("rpi-new-001", "fleet-alpha", "raspberry_pi_5")
        .await;
    assert_eq!(prov_status, StatusCode::CREATED);
    assert_eq!(prov_json["device_id"], "rpi-new-001");

    // Send command to newly provisioned device
    let (cmd_status, cmd_json) = h
        .send_command("rpi-new-001", "fleet-alpha", "show log stats", "admin")
        .await;
    assert_eq!(cmd_status, StatusCode::OK);

    let cmd_id: Uuid = cmd_json["id"].as_str().unwrap().parse().unwrap();

    // Execute on agent
    let envelope: CommandEnvelope =
        serde_json::from_slice(&h.mqtt.published().last().unwrap().payload).unwrap();
    assert_eq!(envelope.device_id, "rpi-new-001");

    let agent_resp = h.agent_execute(&envelope).await;
    assert_eq!(agent_resp.status, CommandStatus::Completed);

    let (resp_status, _) = h.rest_ingest_response(&agent_resp).await;
    assert_eq!(resp_status, StatusCode::OK);

    let record = h.get_command_record(cmd_id).await.unwrap();
    assert!(record.response.is_some());
}
