//! Real-time event types broadcast over WebSocket connections.

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// Server-sent events pushed to WebSocket clients.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsEvent {
    /// A new command was dispatched.
    CommandDispatched {
        command_id: Uuid,
        device_id: String,
        command: String,
        initiated_by: String,
        created_at: DateTime<Utc>,
    },

    /// A command response was received from a device.
    CommandResponse {
        command_id: Uuid,
        device_id: String,
        status: String,
        inference_tier: Option<String>,
        response_text: Option<String>,
        latency_ms: Option<i64>,
        responded_at: DateTime<Utc>,
    },

    /// A device heartbeat was received.
    DeviceHeartbeat {
        device_id: String,
        timestamp: DateTime<Utc>,
    },

    /// Device status changed.
    DeviceStatusChanged {
        device_id: String,
        old_status: String,
        new_status: String,
        changed_at: DateTime<Utc>,
    },

    /// A new device was provisioned.
    DeviceProvisioned {
        device_id: String,
        fleet_id: String,
        hardware_type: String,
        provisioned_at: DateTime<Utc>,
    },

    /// Telemetry readings were ingested.
    TelemetryIngested {
        device_id: String,
        count: usize,
        source: String,
        timestamp: DateTime<Utc>,
    },

    /// A device shadow was updated.
    ShadowUpdated {
        device_id: String,
        shadow_name: String,
        version: u64,
        timestamp: DateTime<Utc>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_serializes_with_type_tag() {
        let event = WsEvent::CommandDispatched {
            command_id: Uuid::nil(),
            device_id: "rpi-001".into(),
            command: "read DTCs".into(),
            initiated_by: "admin".into(),
            created_at: Utc::now(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"command_dispatched""#));
        assert!(json.contains(r#""device_id":"rpi-001""#));
    }

    #[test]
    fn heartbeat_event_serializes() {
        let event = WsEvent::DeviceHeartbeat {
            device_id: "sbc-010".into(),
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"device_heartbeat""#));
    }

    #[test]
    fn command_response_event_serializes() {
        let event = WsEvent::CommandResponse {
            command_id: Uuid::nil(),
            device_id: "rpi-001".into(),
            status: "completed".into(),
            inference_tier: Some("local".into()),
            response_text: Some("No DTCs found".into()),
            latency_ms: Some(45),
            responded_at: Utc::now(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"command_response""#));
        assert!(json.contains("No DTCs found"));
    }

    #[test]
    fn shadow_updated_event_serializes() {
        let event = WsEvent::ShadowUpdated {
            device_id: "rpi-001".into(),
            shadow_name: "diagnostics".into(),
            version: 7,
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"shadow_updated""#));
        assert!(json.contains(r#""shadow_name":"diagnostics""#));
        assert!(json.contains(r#""version":7"#));
    }

    #[test]
    fn status_changed_event_serializes() {
        let event = WsEvent::DeviceStatusChanged {
            device_id: "rpi-002".into(),
            old_status: "online".into(),
            new_status: "offline".into(),
            changed_at: Utc::now(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"device_status_changed""#));
        assert!(json.contains(r#""old_status":"online""#));
    }
}
