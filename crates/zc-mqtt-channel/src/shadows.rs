//! AWS IoT Device Shadow MQTT operations.
//!
//! Provides typed helpers for publishing shadow updates and subscribing
//! to shadow delta notifications via the MQTT channel.

use rumqttc::QoS;

use crate::channel::Channel;
use crate::error::{MqttError, MqttResult};
use zc_protocol::{shadows::ShadowUpdate, topics};

/// Shadow operations backed by a `Channel` implementation.
///
/// Wraps any `Channel` (real or mock) to provide shadow-specific
/// publish and subscribe methods.
pub struct ShadowClient<'a, C: Channel> {
    channel: &'a C,
    fleet_id: String,
    device_id: String,
}

impl<'a, C: Channel> ShadowClient<'a, C> {
    pub fn new(channel: &'a C, fleet_id: impl Into<String>, device_id: impl Into<String>) -> Self {
        Self {
            channel,
            fleet_id: fleet_id.into(),
            device_id: device_id.into(),
        }
    }

    /// Publish a shadow update (reported state).
    pub async fn publish_update(&self, update: &ShadowUpdate) -> MqttResult<()> {
        let topic = topics::shadow_update(&self.fleet_id, &self.device_id);
        let bytes =
            serde_json::to_vec(update).map_err(|e| MqttError::Serialization(e.to_string()))?;
        self.channel.publish(&topic, &bytes, QoS::AtLeastOnce).await
    }

    /// Publish arbitrary reported state as a shadow update.
    pub async fn report_state(
        &self,
        shadow_name: &str,
        reported: serde_json::Value,
        version: u64,
    ) -> MqttResult<()> {
        let update = ShadowUpdate {
            device_id: self.device_id.clone(),
            shadow_name: shadow_name.to_string(),
            reported,
            version,
        };
        self.publish_update(&update).await
    }

    /// Subscribe to shadow delta notifications for this device.
    pub async fn subscribe_delta(&self) -> MqttResult<()> {
        let topic = topics::shadow_delta(&self.fleet_id, &self.device_id);
        self.channel.subscribe(&topic, QoS::AtLeastOnce).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockChannel;
    use serde_json::json;

    #[tokio::test]
    async fn publish_shadow_update() {
        let mock = MockChannel::new();
        let client = ShadowClient::new(&mock, "fleet-alpha", "rpi-001");

        let update = ShadowUpdate {
            device_id: "rpi-001".into(),
            shadow_name: "diagnostics".into(),
            reported: json!({"dtc_count": 3, "firmware": "0.1.0"}),
            version: 1,
        };
        client.publish_update(&update).await.unwrap();

        let msgs = mock.published();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].topic, "fleet/fleet-alpha/rpi-001/shadow/update");
        let payload: ShadowUpdate = serde_json::from_slice(&msgs[0].payload).unwrap();
        assert_eq!(payload.shadow_name, "diagnostics");
        assert_eq!(payload.version, 1);
    }

    #[tokio::test]
    async fn report_state_helper() {
        let mock = MockChannel::new();
        let client = ShadowClient::new(&mock, "fleet-alpha", "rpi-001");

        client
            .report_state("config", json!({"telemetry_interval": 30}), 5)
            .await
            .unwrap();

        let msgs = mock.published();
        assert_eq!(msgs.len(), 1);
        let payload: ShadowUpdate = serde_json::from_slice(&msgs[0].payload).unwrap();
        assert_eq!(payload.shadow_name, "config");
        assert_eq!(payload.reported["telemetry_interval"], 30);
        assert_eq!(payload.version, 5);
    }

    #[tokio::test]
    async fn subscribe_shadow_delta() {
        let mock = MockChannel::new();
        let client = ShadowClient::new(&mock, "fleet-alpha", "rpi-001");

        client.subscribe_delta().await.unwrap();

        assert!(mock.is_subscribed_to("fleet/fleet-alpha/rpi-001/shadow/delta"));
    }
}
