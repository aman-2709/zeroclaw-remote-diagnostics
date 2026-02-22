//! MQTT channel — async client for AWS IoT Core communication.
//!
//! Wraps `rumqttc::AsyncClient` with typed publish helpers for
//! commands, telemetry, heartbeats, and shadow operations.

use async_trait::async_trait;
use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS};
use serde::Serialize;

use crate::config::MqttConfig;
use crate::error::{MqttError, MqttResult};
use crate::tls;
use zc_protocol::{
    TelemetrySource, commands::CommandResponse, device::Heartbeat, telemetry::TelemetryBatch,
    topics,
};

// ── Channel trait ─────────────────────────────────────────────

/// Abstraction for MQTT message publishing and subscribing.
///
/// Enables mocking in tests without a real MQTT broker.
#[async_trait]
pub trait Channel: Send + Sync {
    /// Publish a raw payload to a topic.
    async fn publish(&self, topic: &str, payload: &[u8], qos: QoS) -> MqttResult<()>;

    /// Subscribe to a topic filter.
    async fn subscribe(&self, filter: &str, qos: QoS) -> MqttResult<()>;
}

// ── MqttChannel ───────────────────────────────────────────────

/// MQTT channel connected to AWS IoT Core.
///
/// Owns the `AsyncClient` for publishing/subscribing. The `EventLoop`
/// is returned separately from `new()` — the caller (fleet agent) must
/// drive it in a spawned task via `eventloop.poll()`.
pub struct MqttChannel {
    client: AsyncClient,
    fleet_id: String,
    device_id: String,
}

impl MqttChannel {
    /// Create a new MQTT channel with TLS (production mode).
    ///
    /// Returns `(channel, event_loop)`. The caller must poll the event loop:
    /// ```ignore
    /// tokio::spawn(async move {
    ///     loop {
    ///         if let Err(e) = eventloop.poll().await {
    ///             tracing::error!("MQTT event loop error: {e}");
    ///             tokio::time::sleep(Duration::from_secs(5)).await;
    ///         }
    ///     }
    /// });
    /// ```
    pub fn new(
        config: &MqttConfig,
        fleet_id: impl Into<String>,
        device_id: impl Into<String>,
    ) -> MqttResult<(Self, EventLoop)> {
        let fleet_id = fleet_id.into();
        let device_id = device_id.into();

        let mut options =
            MqttOptions::new(&config.client_id, &config.broker_host, config.broker_port);
        options.set_keep_alive(std::time::Duration::from_secs(config.keepalive_secs.into()));

        let transport = tls::load_tls_transport(config)?;
        options.set_transport(transport);

        let (client, eventloop) = AsyncClient::new(options, 64);

        Ok((
            Self {
                client,
                fleet_id,
                device_id,
            },
            eventloop,
        ))
    }

    /// Create a channel for local development (no TLS).
    pub fn new_plaintext(
        host: &str,
        port: u16,
        client_id: &str,
        fleet_id: impl Into<String>,
        device_id: impl Into<String>,
    ) -> (Self, EventLoop) {
        let mut options = MqttOptions::new(client_id, host, port);
        options.set_keep_alive(std::time::Duration::from_secs(30));

        let (client, eventloop) = AsyncClient::new(options, 64);

        (
            Self {
                client,
                fleet_id: fleet_id.into(),
                device_id: device_id.into(),
            },
            eventloop,
        )
    }

    pub fn fleet_id(&self) -> &str {
        &self.fleet_id
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    // ── Typed publish helpers ─────────────────────────────────

    /// Publish a command response.
    pub async fn publish_response(&self, response: &CommandResponse) -> MqttResult<()> {
        let topic = topics::command_response(&self.fleet_id, &self.device_id);
        self.publish_json(&topic, response).await
    }

    /// Publish a telemetry batch, routing to the correct source topic.
    pub async fn publish_telemetry(&self, batch: &TelemetryBatch) -> MqttResult<()> {
        let topic = if batch.readings.is_empty() {
            topics::telemetry_system(&self.fleet_id, &self.device_id)
        } else {
            match batch.readings[0].source {
                TelemetrySource::Obd2 => topics::telemetry_obd2(&self.fleet_id, &self.device_id),
                TelemetrySource::System => {
                    topics::telemetry_system(&self.fleet_id, &self.device_id)
                }
                TelemetrySource::Canbus => {
                    topics::telemetry_canbus(&self.fleet_id, &self.device_id)
                }
            }
        };
        self.publish_json(&topic, batch).await
    }

    /// Publish a heartbeat.
    pub async fn publish_heartbeat(&self, heartbeat: &Heartbeat) -> MqttResult<()> {
        let topic = topics::heartbeat(&self.fleet_id, &self.device_id);
        self.publish_json(&topic, heartbeat).await
    }

    /// Publish a command acknowledgement.
    pub async fn publish_ack(&self, ack: &serde_json::Value) -> MqttResult<()> {
        let topic = topics::command_ack(&self.fleet_id, &self.device_id);
        self.publish_json(&topic, ack).await
    }

    // ── Subscription helpers ──────────────────────────────────

    /// Subscribe to incoming commands (device-specific + broadcast).
    pub async fn subscribe_commands(&self) -> MqttResult<()> {
        let device_topic = topics::command_request(&self.fleet_id, &self.device_id);
        self.subscribe(&device_topic, QoS::AtLeastOnce).await?;

        let broadcast = topics::broadcast_command(&self.fleet_id);
        self.subscribe(&broadcast, QoS::AtLeastOnce).await
    }

    /// Subscribe to shadow delta notifications.
    pub async fn subscribe_shadow_delta(&self) -> MqttResult<()> {
        let topic = topics::shadow_delta(&self.fleet_id, &self.device_id);
        self.subscribe(&topic, QoS::AtLeastOnce).await
    }

    /// Subscribe to broadcast config updates.
    pub async fn subscribe_config(&self) -> MqttResult<()> {
        let topic = topics::broadcast_config(&self.fleet_id);
        self.subscribe(&topic, QoS::AtLeastOnce).await
    }

    // ── Internal helpers ──────────────────────────────────────

    async fn publish_json<T: Serialize>(&self, topic: &str, payload: &T) -> MqttResult<()> {
        let bytes =
            serde_json::to_vec(payload).map_err(|e| MqttError::Serialization(e.to_string()))?;
        self.publish(topic, &bytes, QoS::AtLeastOnce).await
    }
}

#[async_trait]
impl Channel for MqttChannel {
    async fn publish(&self, topic: &str, payload: &[u8], qos: QoS) -> MqttResult<()> {
        self.client
            .publish(topic, qos, false, payload)
            .await
            .map_err(|e| MqttError::Publish(e.to_string()))
    }

    async fn subscribe(&self, filter: &str, qos: QoS) -> MqttResult<()> {
        self.client
            .subscribe(filter, qos)
            .await
            .map_err(|e| MqttError::Subscribe(e.to_string()))
    }
}
