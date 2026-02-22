//! Incoming message classification for the MQTT event loop.
//!
//! Parses raw MQTT publishes into typed `IncomingMessage` variants
//! so the fleet agent can dispatch them without topic string matching.

use rumqttc::Publish;
use serde_json;

use zc_protocol::commands::CommandEnvelope;
use zc_protocol::shadows::ShadowDelta;
use zc_protocol::topics;

/// A classified incoming MQTT message.
#[derive(Debug)]
pub enum IncomingMessage {
    /// Command request from the cloud (device-specific or broadcast).
    Command(CommandEnvelope),
    /// Shadow delta — desired state diverged from reported.
    ShadowDelta(ShadowDelta),
    /// Config update broadcast for the fleet.
    ConfigUpdate(serde_json::Value),
    /// Unrecognized topic or payload.
    Unknown { topic: String, payload: Vec<u8> },
}

/// Classify a raw MQTT publish into a typed message.
///
/// Uses `zc_protocol::topics::parse_topic` to extract category/action,
/// then attempts JSON deserialization into the appropriate type.
pub fn classify(publish: &Publish) -> IncomingMessage {
    let topic = &publish.topic;
    let payload = &publish.payload;

    let Some(parsed) = topics::parse_topic(topic) else {
        return IncomingMessage::Unknown {
            topic: topic.clone(),
            payload: payload.to_vec(),
        };
    };

    match (parsed.category.as_str(), parsed.action.as_str()) {
        ("command", "request") => match serde_json::from_slice::<CommandEnvelope>(payload) {
            Ok(envelope) => IncomingMessage::Command(envelope),
            Err(_) => IncomingMessage::Unknown {
                topic: topic.clone(),
                payload: payload.to_vec(),
            },
        },
        ("shadow", "delta") => match serde_json::from_slice::<ShadowDelta>(payload) {
            Ok(delta) => IncomingMessage::ShadowDelta(delta),
            Err(_) => IncomingMessage::Unknown {
                topic: topic.clone(),
                payload: payload.to_vec(),
            },
        },
        ("config", "update") => match serde_json::from_slice::<serde_json::Value>(payload) {
            Ok(value) => IncomingMessage::ConfigUpdate(value),
            Err(_) => IncomingMessage::Unknown {
                topic: topic.clone(),
                payload: payload.to_vec(),
            },
        },
        _ => IncomingMessage::Unknown {
            topic: topic.clone(),
            payload: payload.to_vec(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rumqttc::QoS;
    use serde_json::json;

    fn make_publish(topic: &str, payload: &[u8]) -> Publish {
        let mut publish = Publish::new(topic, QoS::AtLeastOnce, payload);
        publish.pkid = 1;
        publish
    }

    #[test]
    fn classify_command_request() {
        let cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "read DTCs", "operator@test.com");
        let payload = serde_json::to_vec(&cmd).unwrap();
        let publish = make_publish("fleet/fleet-alpha/rpi-001/command/request", &payload);
        let msg = classify(&publish);
        assert!(matches!(msg, IncomingMessage::Command(ref e) if e.device_id == "rpi-001"));
    }

    #[test]
    fn classify_broadcast_command() {
        let cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "status check", "admin");
        let payload = serde_json::to_vec(&cmd).unwrap();
        let publish = make_publish("fleet/fleet-alpha/broadcast/command/request", &payload);
        let msg = classify(&publish);
        assert!(matches!(msg, IncomingMessage::Command(_)));
    }

    #[test]
    fn classify_shadow_delta() {
        let delta = zc_protocol::shadows::ShadowDelta {
            device_id: "rpi-001".into(),
            shadow_name: "config".into(),
            delta: json!({"firmware_version": "0.2.0"}),
            version: 5,
            timestamp: chrono::Utc::now(),
        };
        let payload = serde_json::to_vec(&delta).unwrap();
        let publish = make_publish("fleet/fleet-alpha/rpi-001/shadow/delta", &payload);
        let msg = classify(&publish);
        assert!(matches!(msg, IncomingMessage::ShadowDelta(ref d) if d.version == 5));
    }

    #[test]
    fn classify_config_update() {
        let config = json!({"telemetry_interval_secs": 60});
        let payload = serde_json::to_vec(&config).unwrap();
        let publish = make_publish("fleet/fleet-alpha/broadcast/config/update", &payload);
        let msg = classify(&publish);
        assert!(
            matches!(msg, IncomingMessage::ConfigUpdate(ref v) if v["telemetry_interval_secs"] == 60)
        );
    }

    #[test]
    fn classify_unknown_topic() {
        let publish = make_publish("some/random/topic", b"data");
        let msg = classify(&publish);
        assert!(matches!(msg, IncomingMessage::Unknown { .. }));
    }

    #[test]
    fn classify_bad_payload() {
        let publish = make_publish("fleet/fleet-alpha/rpi-001/command/request", b"not-json");
        let msg = classify(&publish);
        assert!(matches!(msg, IncomingMessage::Unknown { .. }));
    }

    #[test]
    fn classify_telemetry_is_unknown() {
        // Telemetry is outbound only — incoming telemetry is not expected
        let publish = make_publish("fleet/fleet-alpha/rpi-001/telemetry/obd2", b"{}");
        let msg = classify(&publish);
        assert!(matches!(msg, IncomingMessage::Unknown { .. }));
    }
}
