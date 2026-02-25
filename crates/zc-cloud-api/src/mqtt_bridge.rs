//! MQTT bridge — subscribes to device messages and dispatches them
//! through the existing API logic (heartbeat, response, telemetry).

use chrono::Utc;
use rumqttc::{Event, Packet, QoS};

use zc_protocol::commands::CommandResponse;
use zc_protocol::device::Heartbeat;
use zc_protocol::shadows::{ShadowDelta, ShadowUpdate};
use zc_protocol::telemetry::TelemetryBatch;
use zc_protocol::topics;

use crate::events::WsEvent;
use crate::state::AppState;

/// Run the MQTT bridge event loop.
///
/// Drives the rumqttc `EventLoop`, classifying incoming publishes and
/// dispatching them through the same business logic as the HTTP endpoints.
pub async fn run(mut eventloop: rumqttc::EventLoop, state: AppState) {
    tracing::info!("mqtt bridge started");

    loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::Publish(publish))) => {
                handle_incoming(&publish.topic, &publish.payload, &state).await;
            }
            Ok(_) => {} // ConnAck, SubAck, PingResp, etc.
            Err(e) => {
                tracing::error!(error = %e, "mqtt event loop error — reconnecting in 5s");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

/// Classify and handle an incoming MQTT publish.
pub async fn handle_incoming(topic: &str, payload: &[u8], state: &AppState) {
    let Some(parsed) = topics::parse_topic(topic) else {
        tracing::debug!(topic = topic, "ignoring unknown mqtt topic");
        return;
    };

    match (parsed.category.as_str(), parsed.action.as_str()) {
        ("command", "response") => {
            handle_command_response(payload, state).await;
        }
        ("heartbeat", "ping") => {
            handle_heartbeat(payload, state).await;
        }
        ("telemetry", _source) => {
            if let Some(device_id) = &parsed.device_id {
                handle_telemetry(device_id, payload, state).await;
            }
        }
        ("shadow", "update") => {
            if let Some(device_id) = &parsed.device_id {
                handle_shadow_update(&parsed.fleet_id, device_id, payload, state).await;
            }
        }
        _ => {
            tracing::debug!(
                topic = topic,
                category = parsed.category,
                action = parsed.action,
                "ignoring unhandled mqtt topic"
            );
        }
    }
}

/// Handle an incoming command response from a device.
async fn handle_command_response(payload: &[u8], state: &AppState) {
    let resp: CommandResponse = match serde_json::from_slice(payload) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "failed to parse command response payload");
            return;
        }
    };

    let command_id = resp.command_id;
    let status_str = serde_json::to_value(resp.status)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| format!("{:?}", resp.status).to_lowercase());

    let inference_tier_str = serde_json::to_value(resp.inference_tier)
        .ok()
        .and_then(|v| v.as_str().map(String::from));

    if let Some(pool) = &state.pool {
        let row = match crate::db::commands::get_by_id(pool, command_id).await {
            Ok(Some(row)) => row,
            Ok(None) => {
                tracing::warn!(command_id = %command_id, "mqtt response for unknown command");
                return;
            }
            Err(e) => {
                tracing::error!(error = %e, "db error looking up command");
                return;
            }
        };

        let latency_ms = (resp.responded_at - row.created_at).num_milliseconds();

        if let Err(e) = crate::db::commands::update_response(
            pool,
            command_id,
            &status_str,
            inference_tier_str.as_deref().unwrap_or("unknown"),
            resp.response_text.as_deref(),
            resp.response_data.as_ref(),
            latency_ms,
            resp.error.as_deref(),
        )
        .await
        {
            tracing::error!(error = %e, "failed to update command response in db");
            return;
        }
    } else {
        let mut commands = state.commands.write().await;
        if let Some(record) = commands.iter_mut().find(|r| r.envelope.id == command_id) {
            record.response = Some(resp.clone());
        } else {
            tracing::warn!(command_id = %command_id, "mqtt response for unknown command (in-memory)");
            return;
        }
    }

    tracing::info!(command_id = %command_id, status = %status_str, "mqtt command response ingested");

    let _ = state.event_tx.send(WsEvent::CommandResponse {
        command_id,
        device_id: resp.device_id,
        status: status_str,
        inference_tier: inference_tier_str,
        response_text: resp.response_text,
        response_data: resp.response_data,
        error: resp.error,
        latency_ms: Some(resp.latency_ms as i64),
        responded_at: Utc::now(),
    });
}

/// Handle an incoming heartbeat from a device.
async fn handle_heartbeat(payload: &[u8], state: &AppState) {
    let hb: Heartbeat = match serde_json::from_slice(payload) {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!(error = %e, "failed to parse heartbeat payload");
            return;
        }
    };

    if let Some(pool) = &state.pool {
        if let Err(e) =
            crate::db::devices::update_heartbeat(pool, &hb.device_id, hb.timestamp).await
        {
            tracing::error!(error = %e, "failed to update heartbeat in db");
        }
    } else {
        let mut devices = state.devices.write().await;
        if let Some(device) = devices.get_mut(&hb.device_id) {
            device.last_heartbeat = Some(hb.timestamp);
        }
    }

    tracing::debug!(device_id = %hb.device_id, "mqtt heartbeat received");

    let _ = state.event_tx.send(WsEvent::DeviceHeartbeat {
        device_id: hb.device_id,
        timestamp: Utc::now(),
    });
}

/// Handle incoming telemetry from a device.
async fn handle_telemetry(device_id: &str, payload: &[u8], state: &AppState) {
    let batch: TelemetryBatch = match serde_json::from_slice(payload) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(error = %e, device_id = device_id, "failed to parse telemetry payload");
            return;
        }
    };

    let count = batch.readings.len();
    let source = batch
        .readings
        .first()
        .map(|r| format!("{:?}", r.source).to_lowercase())
        .unwrap_or_else(|| "unknown".to_string());

    if let Some(pool) = &state.pool {
        let rows: Vec<crate::db::telemetry::TelemetryRow> = batch
            .readings
            .iter()
            .map(|r| crate::db::telemetry::TelemetryRow {
                time: r.time,
                device_id: device_id.to_string(),
                metric_name: r.metric_name.clone(),
                value_numeric: r.value_numeric,
                value_text: r.value_text.clone(),
                value_json: r.value_json.clone(),
                unit: r.unit.clone(),
                source: format!("{:?}", r.source).to_lowercase(),
            })
            .collect();

        if let Err(e) = crate::db::telemetry::insert_batch(pool, &rows).await {
            tracing::error!(error = %e, "failed to insert telemetry batch");
            return;
        }
    }

    tracing::debug!(
        device_id = device_id,
        count = count,
        "mqtt telemetry ingested"
    );

    let _ = state.event_tx.send(WsEvent::TelemetryIngested {
        device_id: device_id.to_string(),
        count,
        source,
        timestamp: Utc::now(),
    });
}

/// Handle an incoming shadow update from a device.
async fn handle_shadow_update(fleet_id: &str, device_id: &str, payload: &[u8], state: &AppState) {
    let update: ShadowUpdate = match serde_json::from_slice(payload) {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!(error = %e, device_id = device_id, "failed to parse shadow update payload");
            return;
        }
    };

    let shadow_name = update.shadow_name.clone();
    let version;

    if let Some(pool) = &state.pool {
        match crate::db::shadows::upsert_reported(pool, device_id, &shadow_name, &update.reported)
            .await
        {
            Ok(row) => {
                version = row.version as u64;
                // Compute delta and publish if non-empty.
                let delta = compute_delta(&row.desired, &row.reported);
                if !delta.as_object().is_none_or(|o| o.is_empty()) {
                    publish_shadow_delta(state, fleet_id, device_id, &shadow_name, delta, version)
                        .await;
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to upsert shadow reported state");
                return;
            }
        }
    } else {
        let mut shadows = state.shadows.write().await;
        let key = (device_id.to_string(), shadow_name.clone());
        let entry = shadows
            .entry(key)
            .or_insert_with(|| zc_protocol::shadows::ShadowState {
                reported: serde_json::Value::Object(Default::default()),
                desired: serde_json::Value::Object(Default::default()),
                version: 0,
                last_updated: Utc::now(),
            });

        // Merge reported state (top-level key replacement).
        if let (Some(existing), Some(incoming)) =
            (entry.reported.as_object_mut(), update.reported.as_object())
        {
            for (k, v) in incoming {
                existing.insert(k.clone(), v.clone());
            }
        }
        entry.version += 1;
        entry.last_updated = Utc::now();
        version = entry.version;

        // Compute delta and publish if non-empty.
        let delta = compute_delta(&entry.desired, &entry.reported);
        if !delta.as_object().is_none_or(|o| o.is_empty()) {
            // Drop the write lock before publishing.
            let delta_clone = delta.clone();
            drop(shadows);
            publish_shadow_delta(
                state,
                fleet_id,
                device_id,
                &shadow_name,
                delta_clone,
                version,
            )
            .await;
        }
    }

    tracing::info!(
        device_id = device_id,
        shadow = shadow_name,
        version = version,
        "shadow update processed"
    );

    let _ = state.event_tx.send(WsEvent::ShadowUpdated {
        device_id: device_id.to_string(),
        shadow_name,
        version,
        timestamp: Utc::now(),
    });
}

/// Compute delta: keys in `desired` that differ from `reported`.
pub(crate) fn compute_delta(
    desired: &serde_json::Value,
    reported: &serde_json::Value,
) -> serde_json::Value {
    let mut delta = serde_json::Map::new();

    if let Some(desired_obj) = desired.as_object() {
        let reported_obj = reported.as_object();
        for (key, desired_val) in desired_obj {
            let reported_val = reported_obj.and_then(|r| r.get(key));
            if reported_val != Some(desired_val) {
                delta.insert(key.clone(), desired_val.clone());
            }
        }
    }

    serde_json::Value::Object(delta)
}

/// Publish a ShadowDelta to the device via MQTT.
async fn publish_shadow_delta(
    state: &AppState,
    fleet_id: &str,
    device_id: &str,
    shadow_name: &str,
    delta: serde_json::Value,
    version: u64,
) {
    let shadow_delta = ShadowDelta {
        device_id: device_id.to_string(),
        shadow_name: shadow_name.to_string(),
        delta,
        version,
        timestamp: Utc::now(),
    };

    if let Some(mqtt) = &state.mqtt {
        let topic = topics::shadow_delta(fleet_id, device_id);
        match serde_json::to_vec(&shadow_delta) {
            Ok(payload) => {
                if let Err(e) = mqtt.publish(&topic, &payload, QoS::AtLeastOnce).await {
                    tracing::error!(error = %e, "failed to publish shadow delta");
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "failed to serialize shadow delta");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> AppState {
        AppState::with_sample_data()
    }

    #[tokio::test]
    async fn handle_heartbeat_message() {
        let state = sample_state();
        let mut rx = state.event_tx.subscribe();

        let hb = Heartbeat {
            device_id: "rpi-001".into(),
            fleet_id: "fleet-alpha".into(),
            status: zc_protocol::device::DeviceStatus::Online,
            uptime_secs: 3600,
            ollama_status: zc_protocol::device::ServiceStatus::Running,
            can_status: zc_protocol::device::ServiceStatus::Running,
            agent_version: "0.1.0".into(),
            timestamp: Utc::now(),
        };

        let payload = serde_json::to_vec(&hb).unwrap();
        let topic = topics::heartbeat("fleet-alpha", "rpi-001");

        handle_incoming(&topic, &payload, &state).await;

        let event = rx.try_recv().unwrap();
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("device_heartbeat"));
        assert!(json.contains("rpi-001"));
    }

    #[tokio::test]
    async fn handle_command_response_message() {
        let state = sample_state();
        let mut rx = state.event_tx.subscribe();

        // Pre-populate a command.
        let cmd_id = uuid::Uuid::now_v7();
        let envelope = zc_protocol::commands::CommandEnvelope {
            id: cmd_id,
            fleet_id: "fleet-alpha".into(),
            device_id: "rpi-001".into(),
            natural_language: "read DTCs".into(),
            parsed_intent: None,
            correlation_id: cmd_id,
            initiated_by: "admin".into(),
            created_at: Utc::now(),
            timeout_secs: 30,
        };
        {
            let mut cmds = state.commands.try_write().unwrap();
            cmds.push(crate::state::CommandRecord {
                envelope,
                response: None,
                created_at: Utc::now(),
            });
        }

        let resp = CommandResponse {
            command_id: cmd_id,
            correlation_id: cmd_id,
            device_id: "rpi-001".into(),
            status: zc_protocol::commands::CommandStatus::Completed,
            inference_tier: zc_protocol::commands::InferenceTier::Local,
            response_text: Some("No DTCs found".into()),
            response_data: None,
            latency_ms: 42,
            responded_at: Utc::now(),
            error: None,
        };

        let payload = serde_json::to_vec(&resp).unwrap();
        let topic = topics::command_response("fleet-alpha", "rpi-001");

        handle_incoming(&topic, &payload, &state).await;

        let event = rx.try_recv().unwrap();
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("command_response"));
        assert!(json.contains("No DTCs found"));

        // Verify in-memory record was updated.
        let commands = state.commands.read().await;
        let record = commands.iter().find(|r| r.envelope.id == cmd_id).unwrap();
        assert!(record.response.is_some());
    }

    #[tokio::test]
    async fn handle_telemetry_message() {
        let state = sample_state();
        let mut rx = state.event_tx.subscribe();

        let batch = TelemetryBatch {
            device_id: "rpi-001".into(),
            readings: vec![zc_protocol::telemetry::TelemetryReading {
                device_id: "rpi-001".into(),
                time: Utc::now(),
                metric_name: "engine_rpm".into(),
                value_numeric: Some(3500.0),
                value_text: None,
                value_json: None,
                unit: Some("rpm".into()),
                source: zc_protocol::TelemetrySource::Obd2,
            }],
            collected_at: Utc::now(),
        };

        let payload = serde_json::to_vec(&batch).unwrap();
        let topic = topics::telemetry_obd2("fleet-alpha", "rpi-001");

        handle_incoming(&topic, &payload, &state).await;

        let event = rx.try_recv().unwrap();
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("telemetry_ingested"));
        assert!(json.contains("rpi-001"));
    }

    #[tokio::test]
    async fn handle_unknown_topic() {
        let state = sample_state();
        let mut rx = state.event_tx.subscribe();

        handle_incoming("some/random/topic", b"data", &state).await;

        // No event should be broadcast.
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn handle_malformed_payload() {
        let state = sample_state();
        let mut rx = state.event_tx.subscribe();

        let topic = topics::heartbeat("fleet-alpha", "rpi-001");
        handle_incoming(&topic, b"not-json", &state).await;

        // No event should be broadcast for malformed data.
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn handle_shadow_update_stores_reported() {
        let state = sample_state();
        let mut rx = state.event_tx.subscribe();

        let update = zc_protocol::shadows::ShadowUpdate {
            device_id: "rpi-001".into(),
            shadow_name: "diagnostics".into(),
            reported: serde_json::json!({"firmware": "0.1.0", "uptime": 120}),
            version: 1,
        };

        let payload = serde_json::to_vec(&update).unwrap();
        let topic = topics::shadow_update("fleet-alpha", "rpi-001");
        handle_incoming(&topic, &payload, &state).await;

        // Verify in-memory shadow was stored.
        let shadows = state.shadows.read().await;
        let shadow = shadows
            .get(&("rpi-001".to_string(), "diagnostics".to_string()))
            .unwrap();
        assert_eq!(shadow.reported["firmware"], "0.1.0");
        assert_eq!(shadow.version, 1);

        // Verify broadcast event.
        let event = rx.try_recv().unwrap();
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("shadow_updated"));
        assert!(json.contains("diagnostics"));
    }

    #[tokio::test]
    async fn handle_shadow_update_broadcasts_event() {
        let state = sample_state();
        let mut rx = state.event_tx.subscribe();

        let update = zc_protocol::shadows::ShadowUpdate {
            device_id: "rpi-001".into(),
            shadow_name: "config".into(),
            reported: serde_json::json!({"mode": "normal"}),
            version: 1,
        };

        let payload = serde_json::to_vec(&update).unwrap();
        let topic = topics::shadow_update("fleet-alpha", "rpi-001");
        handle_incoming(&topic, &payload, &state).await;

        let event = rx.try_recv().unwrap();
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"shadow_updated""#));
        assert!(json.contains(r#""device_id":"rpi-001""#));
    }

    #[test]
    fn compute_delta_finds_differences() {
        let desired = serde_json::json!({"firmware": "0.2.0", "mode": "debug"});
        let reported = serde_json::json!({"firmware": "0.1.0", "mode": "debug"});
        let delta = compute_delta(&desired, &reported);
        assert_eq!(delta["firmware"], "0.2.0");
        assert!(delta.get("mode").is_none());
    }

    #[test]
    fn compute_delta_empty_when_matching() {
        let state = serde_json::json!({"firmware": "0.2.0"});
        let delta = compute_delta(&state, &state);
        assert!(delta.as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn shadow_update_publishes_delta_when_desired_differs() {
        let mqtt = std::sync::Arc::new(zc_mqtt_channel::MockChannel::new());
        let mut state = sample_state();
        state.mqtt = Some(mqtt.clone());

        // Pre-set desired state.
        {
            let mut shadows = state.shadows.write().await;
            shadows.insert(
                ("rpi-001".to_string(), "config".to_string()),
                zc_protocol::shadows::ShadowState {
                    reported: serde_json::json!({}),
                    desired: serde_json::json!({"firmware": "0.2.0"}),
                    version: 0,
                    last_updated: Utc::now(),
                },
            );
        }

        let update = zc_protocol::shadows::ShadowUpdate {
            device_id: "rpi-001".into(),
            shadow_name: "config".into(),
            reported: serde_json::json!({"firmware": "0.1.0"}),
            version: 1,
        };

        let payload = serde_json::to_vec(&update).unwrap();
        let topic = topics::shadow_update("fleet-alpha", "rpi-001");
        handle_incoming(&topic, &payload, &state).await;

        // Verify a delta was published via MQTT.
        let delta_msgs = mqtt.published_to("fleet/fleet-alpha/rpi-001/shadow/delta");
        assert_eq!(delta_msgs.len(), 1);
        let delta: zc_protocol::shadows::ShadowDelta =
            serde_json::from_slice(&delta_msgs[0].payload).unwrap();
        assert_eq!(delta.delta["firmware"], "0.2.0");
    }
}
