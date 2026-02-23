//! MQTT event loop driver and incoming message dispatcher.
//!
//! Drives the rumqttc event loop in a loop, extracting incoming
//! publishes and dispatching them through the command executor.

use rumqttc::{Event, EventLoop, Packet};

use zc_canbus_tools::CanInterface;
use zc_log_tools::LogSource;
use zc_mqtt_channel::{Channel, IncomingMessage, MqttChannel, ShadowClient, classify};
use zc_protocol::commands::CommandStatus;

use crate::executor::CommandExecutor;
use crate::inference::OllamaClient;
use crate::registry::ToolRegistry;
use crate::shadow_sync::SharedShadowState;

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
}
