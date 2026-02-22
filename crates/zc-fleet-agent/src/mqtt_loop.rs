//! MQTT event loop driver and incoming message dispatcher.
//!
//! Drives the rumqttc event loop in a loop, extracting incoming
//! publishes and dispatching them through the command executor.

use rumqttc::{Event, EventLoop, Packet};

use zc_canbus_tools::CanInterface;
use zc_log_tools::LogSource;
use zc_mqtt_channel::{IncomingMessage, MqttChannel, classify};
use zc_protocol::commands::CommandStatus;

use crate::executor::CommandExecutor;
use crate::inference::OllamaClient;
use crate::registry::ToolRegistry;

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
) {
    let executor = CommandExecutor::new(registry, can_interface, log_source, ollama);

    loop {
        match eventloop.poll().await {
            Ok(event) => {
                if let Event::Incoming(Packet::Publish(publish)) = event {
                    let msg = classify(&publish);
                    handle_message(msg, channel, &executor).await;
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
            tracing::info!(
                shadow = %delta.shadow_name,
                version = delta.version,
                "received shadow delta (handling not yet implemented)"
            );
            // Phase 2: Apply shadow delta to local config
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
