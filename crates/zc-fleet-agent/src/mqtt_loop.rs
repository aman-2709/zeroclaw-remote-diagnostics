//! MQTT event loop driver and incoming message dispatcher.
//!
//! Drives the rumqttc event loop in a loop, extracting incoming
//! publishes and dispatching them through the command executor.

use rumqttc::{Event, EventLoop, Packet};

use zc_canbus_tools::CanInterface;
use zc_log_tools::LogSource;
use zc_mqtt_channel::{Channel, IncomingMessage, MqttChannel, ShadowClient, classify};
use zc_protocol::commands::{CommandResponse, CommandStatus};

use crate::executor::CommandExecutor;
use crate::inference::OllamaClient;
use crate::registry::ToolRegistry;
use crate::shadow_sync::SharedShadowState;

/// Maximum MQTT payload size in bytes.
/// AWS IoT Core supports 128 KB payloads. We use 128 KB minus headroom
/// for MQTT packet headers and topic strings.
const MAX_MQTT_PAYLOAD: usize = 128 * 1024;

/// Drive the MQTT event loop and dispatch incoming messages.
///
/// Runs forever until the event loop returns an unrecoverable error or
/// the task is cancelled. Intended to be spawned as a background tokio task.
pub async fn run(
    mut eventloop: EventLoop,
    channel: &MqttChannel,
    registry: &ToolRegistry,
    can_interface: &dyn CanInterface,
    log_source: &dyn LogSource,
    ollama: Option<&OllamaClient>,
    shadow_state: &SharedShadowState,
) {
    let executor = CommandExecutor::new(registry, can_interface, log_source, ollama);
    let shadow_client = ShadowClient::new(channel, channel.fleet_id(), channel.device_id());

    loop {
        match eventloop.poll().await {
            Ok(event) => {
                if let Event::Incoming(Packet::Publish(publish)) = event {
                    let msg = classify(&publish);
                    handle_message(msg, channel, &executor, shadow_state, &shadow_client).await;
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "MQTT event loop error, reconnecting in 5s");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

async fn handle_message(
    msg: IncomingMessage,
    channel: &MqttChannel,
    executor: &CommandExecutor<'_>,
    shadow_state: &SharedShadowState,
    shadow_client: &ShadowClient<'_, MqttChannel>,
) {
    match msg {
        IncomingMessage::Command(envelope) => {
            tracing::info!(
                command_id = %envelope.id,
                from = %envelope.initiated_by,
                "received command"
            );

            // Send acknowledgement
            let ack = serde_json::json!({
                "command_id": envelope.id,
                "status": "processing",
            });
            if let Err(e) = channel.publish_ack(&ack).await {
                tracing::warn!(error = %e, "failed to publish ack");
            }

            // Execute the command
            let response = executor.execute(&envelope).await;

            // Update shadow state with last command info.
            {
                let mut state = shadow_state.write().await;
                state.last_command_id = Some(envelope.id.to_string());
                state.last_command_tool = response
                    .response_data
                    .as_ref()
                    .and_then(|d| d.get("tool_name"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                state.last_command_at = Some(chrono::Utc::now().to_rfc3339());
            }

            match response.status {
                CommandStatus::Completed => {
                    tracing::info!(
                        command_id = %envelope.id,
                        latency_ms = response.latency_ms,
                        "command completed"
                    );
                }
                _ => {
                    tracing::warn!(
                        command_id = %envelope.id,
                        error = ?response.error,
                        "command failed"
                    );
                }
            }

            // Cap response size to fit MQTT packet limit before publishing
            let response = cap_response_size(response);

            // Publish response back
            if let Err(e) = channel.publish_response(&response).await {
                tracing::error!(error = %e, "failed to publish command response");
            }
        }
        IncomingMessage::ShadowDelta(delta) => {
            handle_shadow_delta(&delta, shadow_client).await;
        }
        IncomingMessage::ConfigUpdate(config) => {
            tracing::info!("received config update (handling not yet implemented)");
            tracing::debug!(config = %config, "config payload");
            // Phase 2: Apply runtime config changes
        }
        IncomingMessage::Unknown { topic, .. } => {
            tracing::debug!(topic = %topic, "ignoring unrecognized message");
        }
    }
}

/// Ensure the serialized response fits within the MQTT packet limit.
///
/// If the response exceeds [`MAX_MQTT_PAYLOAD`], truncates `response_data`
/// first (it's the only unbounded field — shell output is already capped
/// at 8 KB by `shell.rs`). Falls back to dropping `response_data` entirely
/// and summarising in `response_text`.
fn cap_response_size(mut response: CommandResponse) -> CommandResponse {
    let Ok(bytes) = serde_json::to_vec(&response) else {
        return response;
    };

    if bytes.len() <= MAX_MQTT_PAYLOAD {
        return response;
    }

    let original_len = bytes.len();

    // Strategy 1: If response_data has a "data.entries" array (log tools),
    // trim entries from the front until it fits.
    let has_entries = response
        .response_data
        .as_ref()
        .and_then(|d| d.get("data"))
        .and_then(|d| d.get("entries"))
        .and_then(|e| e.as_array())
        .is_some_and(|a| a.len() > 1);

    if has_entries {
        let mut data = response.response_data.take().unwrap();

        // Extract the entries array so we can mutate it freely.
        let mut entries = data["data"]["entries"].as_array().cloned().unwrap();
        let original_count = entries.len();

        // Estimate bytes to skip close to target in one jump
        let excess = original_len - MAX_MQTT_PAYLOAD;
        let bytes_per_entry = original_len / original_count;
        let skip = (excess / bytes_per_entry).min(entries.len() - 1);
        if skip > 0 {
            entries.drain(..skip);
        }

        // Fine-tune: remove oldest entries one at a time
        loop {
            data["data"]["entries"] = serde_json::Value::Array(entries.clone());
            data["data"]["shown"] = serde_json::json!(entries.len());
            response.response_data = Some(data.clone());

            if serde_json::to_vec(&response).is_ok_and(|b| b.len() <= MAX_MQTT_PAYLOAD) {
                tracing::info!(
                    command_id = %response.command_id,
                    original_entries = original_count,
                    kept_entries = entries.len(),
                    "trimmed log entries to fit MQTT payload"
                );
                return response;
            }
            if entries.len() <= 1 {
                break;
            }
            entries.remove(0);
        }

        // Couldn't fit even with 1 entry — fall through to nuclear option.
        response.response_data = Some(data);
    }

    // Strategy 2 (fallback): Drop response_data entirely, keep summary in response_text.
    if let Some(data) = response.response_data.take() {
        let tool_name = data
            .get("tool_name")
            .and_then(|v| v.as_str())
            .unwrap_or("tool");
        let summary = data
            .get("summary")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        response.response_data = Some(serde_json::json!({
            "truncated": true,
            "original_bytes": original_len,
        }));

        if let Some(s) = summary {
            response.response_text = Some(format!(
                "{tool_name}: {s} [response truncated from {original_len}B]"
            ));
        } else {
            let existing = response.response_text.unwrap_or_default();
            response.response_text = Some(format!(
                "{existing} [response truncated from {original_len}B]"
            ));
        }

        tracing::warn!(
            command_id = %response.command_id,
            original_bytes = original_len,
            "response truncated to fit MQTT packet limit"
        );
    }

    response
}

/// Handle an incoming shadow delta from the cloud.
///
/// For the "config" shadow, logs applied keys and acknowledges via ShadowClient.
/// Unknown shadow names are logged and ignored.
async fn handle_shadow_delta<C: Channel>(
    delta: &zc_protocol::shadows::ShadowDelta,
    shadow_client: &ShadowClient<'_, C>,
) {
    match delta.shadow_name.as_str() {
        "config" => {
            if let Some(obj) = delta.delta.as_object() {
                let keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
                tracing::info!(
                    shadow = "config",
                    version = delta.version,
                    keys = ?keys,
                    "applying config shadow delta"
                );
            }

            // Acknowledge by reporting the delta values as our reported state.
            if let Err(e) = shadow_client
                .report_state("config", delta.delta.clone(), delta.version)
                .await
            {
                tracing::warn!(error = %e, "failed to acknowledge config shadow delta");
            }
        }
        other => {
            tracing::debug!(
                shadow = other,
                version = delta.version,
                "ignoring delta for unknown shadow"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zc_mqtt_channel::MockChannel;
    use zc_protocol::commands::{CommandEnvelope, CommandResponse, InferenceTier};
    use zc_protocol::shadows::ShadowDelta;

    #[tokio::test]
    async fn delta_acknowledge_publishes_report() {
        let mock = MockChannel::new();
        let client = ShadowClient::new(&mock, "fleet-alpha", "rpi-001");

        let delta = ShadowDelta {
            device_id: "rpi-001".into(),
            shadow_name: "config".into(),
            delta: serde_json::json!({"firmware": "0.2.0"}),
            version: 5,
            timestamp: chrono::Utc::now(),
        };

        handle_shadow_delta(&delta, &client).await;

        let msgs = mock.published();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].topic, "fleet/fleet-alpha/rpi-001/shadow/update");
        let update: zc_protocol::shadows::ShadowUpdate =
            serde_json::from_slice(&msgs[0].payload).unwrap();
        assert_eq!(update.shadow_name, "config");
        assert_eq!(update.reported["firmware"], "0.2.0");
    }

    #[tokio::test]
    async fn unknown_shadow_ignored() {
        let mock = MockChannel::new();
        let client = ShadowClient::new(&mock, "fleet-alpha", "rpi-001");

        let delta = ShadowDelta {
            device_id: "rpi-001".into(),
            shadow_name: "unknown-shadow".into(),
            delta: serde_json::json!({"foo": "bar"}),
            version: 1,
            timestamp: chrono::Utc::now(),
        };

        handle_shadow_delta(&delta, &client).await;

        // No message should be published for unknown shadows.
        assert!(mock.published().is_empty());
    }

    // ── cap_response_size tests ─────────────────────────────────

    fn make_response(data: Option<serde_json::Value>) -> CommandResponse {
        let envelope = CommandEnvelope::new("fleet-alpha", "rpi-001", "tail logs", "admin");
        CommandResponse {
            command_id: envelope.id,
            correlation_id: envelope.correlation_id,
            device_id: "rpi-001".into(),
            status: CommandStatus::Completed,
            inference_tier: InferenceTier::Local,
            response_text: Some("Tool 'tail_logs' executed successfully".into()),
            response_data: data,
            latency_ms: 100,
            responded_at: chrono::Utc::now(),
            error: None,
        }
    }

    #[test]
    fn small_response_passes_through() {
        let resp = make_response(Some(
            serde_json::json!({"tool_name": "log_stats", "lines": 42}),
        ));
        let capped = cap_response_size(resp.clone());
        assert_eq!(
            serde_json::to_vec(&capped).unwrap().len(),
            serde_json::to_vec(&resp).unwrap().len()
        );
    }

    #[test]
    fn oversized_entries_are_trimmed() {
        // Build a response_data with entries that exceed 128KB
        let entries: Vec<serde_json::Value> = (0..1500)
            .map(|i| {
                serde_json::json!({
                    "line": i,
                    "message": format!("Feb 23 01:34:{:02} xl4 syslog[1234]: {}", i % 60, "x".repeat(100)),
                    "severity": "info",
                    "source": null,
                    "timestamp": null,
                })
            })
            .collect();
        let big_data = serde_json::json!({
            "tool_name": "tail_logs",
            "summary": "Last 1500 lines from /var/log/syslog",
            "success": true,
            "data": {
                "entries": entries,
                "shown": 1500,
                "total_entries": 50000,
                "path": "/var/log/syslog",
            }
        });
        let resp = make_response(Some(big_data));
        let original_bytes = serde_json::to_vec(&resp).unwrap().len();
        assert!(
            original_bytes > MAX_MQTT_PAYLOAD,
            "test data must exceed limit: {original_bytes}"
        );

        let capped = cap_response_size(resp);

        let capped_bytes = serde_json::to_vec(&capped).unwrap().len();
        assert!(
            capped_bytes <= MAX_MQTT_PAYLOAD,
            "capped response must fit: {capped_bytes}"
        );

        // Should have kept some entries (trimmed, not nuked)
        let data = capped.response_data.unwrap();
        let kept = data["data"]["entries"].as_array().unwrap().len();
        assert!(kept > 0, "should keep some entries");
        assert!(kept < 1500, "should have trimmed: kept {kept}");
        // "shown" metadata should reflect trimmed count
        assert_eq!(data["data"]["shown"], kept);
    }

    #[test]
    fn oversized_non_entries_falls_back_to_nuke() {
        // response_data without entries array — fallback to nuclear truncation
        let big_data = serde_json::json!({
            "tool_name": "tail_logs",
            "summary": "Last 100 lines from /var/log/syslog",
            "success": true,
            "data": {
                "lines": vec!["x".repeat(200); 1000],
            }
        });
        let resp = make_response(Some(big_data));
        let original_bytes = serde_json::to_vec(&resp).unwrap().len();
        assert!(
            original_bytes > MAX_MQTT_PAYLOAD,
            "test data must exceed limit: {original_bytes}"
        );

        let capped = cap_response_size(resp);

        let capped_bytes = serde_json::to_vec(&capped).unwrap().len();
        assert!(
            capped_bytes <= MAX_MQTT_PAYLOAD,
            "capped response must fit: {capped_bytes}"
        );

        // Should have fallback truncation marker
        let data = capped.response_data.unwrap();
        assert_eq!(data["truncated"], true);

        let text = capped.response_text.unwrap();
        assert!(text.contains("tail_logs"));
        assert!(text.contains("truncated"));
    }

    #[test]
    fn no_response_data_not_affected() {
        let resp = make_response(None);
        let capped = cap_response_size(resp.clone());
        assert_eq!(capped.response_text, resp.response_text);
        assert!(capped.response_data.is_none());
    }
}
