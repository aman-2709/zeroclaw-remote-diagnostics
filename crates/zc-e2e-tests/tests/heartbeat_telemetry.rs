//! E2E tests for heartbeat and telemetry flows across MQTT bridge and REST API.

mod helpers;

use axum::http::StatusCode;
use chrono::Utc;
use serde_json::json;

use helpers::TestHarness;
use zc_protocol::commands::{CommandEnvelope, CommandStatus};
use zc_protocol::device::{DeviceStatus, Heartbeat, ServiceStatus};
use zc_protocol::telemetry::{TelemetryBatch, TelemetryReading, TelemetrySource};

/// MQTT heartbeat updates device state and broadcasts event.
#[tokio::test]
async fn e2e_heartbeat_updates_device() {
    let h = TestHarness::with_sample_data();
    let mut rx = h.cloud_state.event_tx.subscribe();

    let hb = Heartbeat {
        device_id: "rpi-001".into(),
        fleet_id: "fleet-alpha".into(),
        status: DeviceStatus::Online,
        uptime_secs: 7200,
        ollama_status: ServiceStatus::Running,
        can_status: ServiceStatus::Running,
        agent_version: "0.1.0".into(),
        timestamp: Utc::now(),
    };

    // Ingest via MQTT bridge path
    let topic = zc_protocol::topics::heartbeat("fleet-alpha", "rpi-001");
    let payload = serde_json::to_vec(&hb).unwrap();
    zc_cloud_api::mqtt_bridge::handle_incoming(&topic, &payload, &h.cloud_state).await;

    // Verify event was broadcast
    let event = rx.try_recv().unwrap();
    let event_json = serde_json::to_string(&event).unwrap();
    assert!(event_json.contains("device_heartbeat"));
    assert!(event_json.contains("rpi-001"));

    // Verify device's last_heartbeat was updated
    let devices = h.cloud_state.devices.read().await;
    let device = devices.get("rpi-001").unwrap();
    assert!(device.last_heartbeat.is_some());
}

/// REST heartbeat endpoint processes heartbeat and broadcasts event.
#[tokio::test]
async fn e2e_heartbeat_via_rest() {
    let h = TestHarness::with_sample_data();
    let mut rx = h.cloud_state.event_tx.subscribe();

    let hb = Heartbeat {
        device_id: "rpi-002".into(),
        fleet_id: "fleet-alpha".into(),
        status: DeviceStatus::Online,
        uptime_secs: 3600,
        ollama_status: ServiceStatus::Unknown,
        can_status: ServiceStatus::Stopped,
        agent_version: "0.1.0".into(),
        timestamp: Utc::now(),
    };

    let (status, json) = h.rest_heartbeat(&hb).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "ok");

    // Verify event was broadcast
    let event = rx.try_recv().unwrap();
    let event_json = serde_json::to_string(&event).unwrap();
    assert!(event_json.contains("device_heartbeat"));
    assert!(event_json.contains("rpi-002"));
}

/// MQTT telemetry batch is ingested and broadcasts event.
#[tokio::test]
async fn e2e_telemetry_via_mqtt() {
    let h = TestHarness::with_sample_data();
    let mut rx = h.cloud_state.event_tx.subscribe();

    let batch = TelemetryBatch {
        device_id: "rpi-001".into(),
        readings: vec![
            TelemetryReading {
                device_id: "rpi-001".into(),
                time: Utc::now(),
                metric_name: "engine_rpm".into(),
                value_numeric: Some(3500.0),
                value_text: None,
                value_json: None,
                unit: Some("rpm".into()),
                source: TelemetrySource::Obd2,
            },
            TelemetryReading {
                device_id: "rpi-001".into(),
                time: Utc::now(),
                metric_name: "coolant_temp".into(),
                value_numeric: Some(90.0),
                value_text: None,
                value_json: None,
                unit: Some("celsius".into()),
                source: TelemetrySource::Obd2,
            },
        ],
        collected_at: Utc::now(),
    };

    let topic = zc_protocol::topics::telemetry_obd2("fleet-alpha", "rpi-001");
    let payload = serde_json::to_vec(&batch).unwrap();
    zc_cloud_api::mqtt_bridge::handle_incoming(&topic, &payload, &h.cloud_state).await;

    // Verify event was broadcast
    let event = rx.try_recv().unwrap();
    let event_json = serde_json::to_string(&event).unwrap();
    assert!(event_json.contains("telemetry_ingested"));
    assert!(event_json.contains("rpi-001"));
    // In-memory mode doesn't store telemetry, but the event carries the count
    assert!(event_json.contains("2") || event_json.contains("\"count\":2"));
}

/// REST telemetry ingestion processes readings and broadcasts event.
#[tokio::test]
async fn e2e_telemetry_via_rest() {
    let h = TestHarness::with_sample_data();
    let mut rx = h.cloud_state.event_tx.subscribe();

    let readings = json!([
        {
            "metric_name": "cpu_usage",
            "value_numeric": 45.2,
            "unit": "percent",
            "source": "system"
        },
        {
            "metric_name": "memory_usage",
            "value_numeric": 512.0,
            "unit": "MB",
            "source": "system"
        }
    ]);

    let (status, json) = h.rest_ingest_telemetry("rpi-001", readings).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["count"], 2);

    // Verify event was broadcast
    let event = rx.try_recv().unwrap();
    let event_json = serde_json::to_string(&event).unwrap();
    assert!(event_json.contains("telemetry_ingested"));
}

/// Full device lifecycle: provision → heartbeat → command → response.
#[tokio::test]
async fn e2e_full_device_lifecycle() {
    let h = TestHarness::empty();

    // 1. Provision device
    let (prov_status, _) = h
        .provision_device("edge-001", "fleet-gamma", "raspberry_pi_4")
        .await;
    assert_eq!(prov_status, StatusCode::CREATED);

    // 2. Send heartbeat
    let hb = Heartbeat {
        device_id: "edge-001".into(),
        fleet_id: "fleet-gamma".into(),
        status: DeviceStatus::Online,
        uptime_secs: 60,
        ollama_status: ServiceStatus::Running,
        can_status: ServiceStatus::Stopped,
        agent_version: "0.1.0".into(),
        timestamp: Utc::now(),
    };
    let (hb_status, _) = h.rest_heartbeat(&hb).await;
    assert_eq!(hb_status, StatusCode::OK);

    // 3. Verify device has updated heartbeat
    {
        let devices = h.cloud_state.devices.read().await;
        let device = devices.get("edge-001").unwrap();
        assert!(device.last_heartbeat.is_some());
    }

    // 4. Send command
    let (cmd_status, cmd_json) = h
        .send_command("edge-001", "fleet-gamma", "show log stats", "admin")
        .await;
    assert_eq!(cmd_status, StatusCode::OK);
    let cmd_id: uuid::Uuid = cmd_json["id"].as_str().unwrap().parse().unwrap();

    // 5. Execute on agent and ingest response
    let envelope: CommandEnvelope =
        serde_json::from_slice(&h.mqtt.published().last().unwrap().payload).unwrap();
    assert_eq!(envelope.device_id, "edge-001");

    let agent_resp = h.agent_execute(&envelope).await;
    let (resp_status, _) = h.rest_ingest_response(&agent_resp).await;
    assert_eq!(resp_status, StatusCode::OK);

    // 6. Verify command completed
    let record = h.get_command_record(cmd_id).await.unwrap();
    assert!(record.response.is_some());
    assert_eq!(record.response.unwrap().status, CommandStatus::Completed);
}
