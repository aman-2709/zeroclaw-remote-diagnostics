//! MQTT topic builders and parsers for the fleet topic hierarchy.
//!
//! Topic structure:
//! ```text
//! fleet/{fleet_id}/{device_id}/command/request
//! fleet/{fleet_id}/{device_id}/command/response
//! fleet/{fleet_id}/{device_id}/command/ack
//! fleet/{fleet_id}/{device_id}/telemetry/{source}
//! fleet/{fleet_id}/{device_id}/shadow/update
//! fleet/{fleet_id}/{device_id}/shadow/delta
//! fleet/{fleet_id}/{device_id}/heartbeat/ping
//! fleet/{fleet_id}/{device_id}/alert/notify
//! fleet/{fleet_id}/broadcast/command/request
//! fleet/{fleet_id}/broadcast/config/update
//! ```

const PREFIX: &str = "fleet";

// ─── Command topics ───

pub fn command_request(fleet_id: &str, device_id: &str) -> String {
    format!("{PREFIX}/{fleet_id}/{device_id}/command/request")
}

pub fn command_response(fleet_id: &str, device_id: &str) -> String {
    format!("{PREFIX}/{fleet_id}/{device_id}/command/response")
}

pub fn command_ack(fleet_id: &str, device_id: &str) -> String {
    format!("{PREFIX}/{fleet_id}/{device_id}/command/ack")
}

// ─── Telemetry topics ───

pub fn telemetry_obd2(fleet_id: &str, device_id: &str) -> String {
    format!("{PREFIX}/{fleet_id}/{device_id}/telemetry/obd2")
}

pub fn telemetry_system(fleet_id: &str, device_id: &str) -> String {
    format!("{PREFIX}/{fleet_id}/{device_id}/telemetry/system")
}

pub fn telemetry_canbus(fleet_id: &str, device_id: &str) -> String {
    format!("{PREFIX}/{fleet_id}/{device_id}/telemetry/canbus")
}

// ─── Shadow topics ───

pub fn shadow_update(fleet_id: &str, device_id: &str) -> String {
    format!("{PREFIX}/{fleet_id}/{device_id}/shadow/update")
}

pub fn shadow_delta(fleet_id: &str, device_id: &str) -> String {
    format!("{PREFIX}/{fleet_id}/{device_id}/shadow/delta")
}

// ─── Heartbeat & alert ───

pub fn heartbeat(fleet_id: &str, device_id: &str) -> String {
    format!("{PREFIX}/{fleet_id}/{device_id}/heartbeat/ping")
}

pub fn alert(fleet_id: &str, device_id: &str) -> String {
    format!("{PREFIX}/{fleet_id}/{device_id}/alert/notify")
}

// ─── Broadcast topics ───

pub fn broadcast_command(fleet_id: &str) -> String {
    format!("{PREFIX}/{fleet_id}/broadcast/command/request")
}

pub fn broadcast_config(fleet_id: &str) -> String {
    format!("{PREFIX}/{fleet_id}/broadcast/config/update")
}

// ─── Subscription patterns (with MQTT wildcards) ───

/// Subscribe to all topics for a specific device.
pub fn device_subscribe_all(fleet_id: &str, device_id: &str) -> String {
    format!("{PREFIX}/{fleet_id}/{device_id}/#")
}

/// Subscribe to all command requests in a fleet (for cloud bridge).
pub fn fleet_command_responses(fleet_id: &str) -> String {
    format!("{PREFIX}/{fleet_id}/+/command/response")
}

/// Subscribe to all heartbeats in a fleet.
pub fn fleet_heartbeats(fleet_id: &str) -> String {
    format!("{PREFIX}/{fleet_id}/+/heartbeat/ping")
}

/// Subscribe to all telemetry of a given source in a fleet.
pub fn fleet_telemetry(fleet_id: &str, source: &str) -> String {
    format!("{PREFIX}/{fleet_id}/+/telemetry/{source}")
}

/// Subscribe to all device shadow updates in a fleet (for cloud bridge).
pub fn fleet_shadow_updates(fleet_id: &str) -> String {
    format!("{PREFIX}/{fleet_id}/+/shadow/update")
}

// ─── Topic parsing ───

/// Parsed MQTT topic components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTopic {
    pub fleet_id: String,
    pub device_id: Option<String>,
    pub category: String,
    pub action: String,
}

/// Parse a topic string into its components.
/// Returns `None` if the topic doesn't match the expected format.
pub fn parse_topic(topic: &str) -> Option<ParsedTopic> {
    let parts: Vec<&str> = topic.split('/').collect();

    if parts.first() != Some(&"fleet") || parts.len() < 4 {
        return None;
    }

    let fleet_id = parts[1].to_string();

    // Broadcast topic: fleet/{fleet_id}/broadcast/{category}/{action}
    if parts[2] == "broadcast" && parts.len() >= 5 {
        return Some(ParsedTopic {
            fleet_id,
            device_id: None,
            category: parts[3].to_string(),
            action: parts[4].to_string(),
        });
    }

    // Device topic: fleet/{fleet_id}/{device_id}/{category}/{action}
    if parts.len() >= 5 {
        return Some(ParsedTopic {
            fleet_id,
            device_id: Some(parts[2].to_string()),
            category: parts[3].to_string(),
            action: parts[4].to_string(),
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_request_topic() {
        assert_eq!(
            command_request("fleet-alpha", "rpi-001"),
            "fleet/fleet-alpha/rpi-001/command/request"
        );
    }

    #[test]
    fn command_response_topic() {
        assert_eq!(
            command_response("fleet-alpha", "rpi-001"),
            "fleet/fleet-alpha/rpi-001/command/response"
        );
    }

    #[test]
    fn telemetry_topics() {
        assert_eq!(
            telemetry_obd2("fleet-alpha", "rpi-001"),
            "fleet/fleet-alpha/rpi-001/telemetry/obd2"
        );
        assert_eq!(
            telemetry_system("fleet-alpha", "rpi-001"),
            "fleet/fleet-alpha/rpi-001/telemetry/system"
        );
        assert_eq!(
            telemetry_canbus("fleet-alpha", "rpi-001"),
            "fleet/fleet-alpha/rpi-001/telemetry/canbus"
        );
    }

    #[test]
    fn shadow_topics() {
        assert_eq!(
            shadow_update("fleet-alpha", "rpi-001"),
            "fleet/fleet-alpha/rpi-001/shadow/update"
        );
        assert_eq!(
            shadow_delta("fleet-alpha", "rpi-001"),
            "fleet/fleet-alpha/rpi-001/shadow/delta"
        );
    }

    #[test]
    fn heartbeat_topic() {
        assert_eq!(
            heartbeat("fleet-alpha", "rpi-001"),
            "fleet/fleet-alpha/rpi-001/heartbeat/ping"
        );
    }

    #[test]
    fn broadcast_topics() {
        assert_eq!(
            broadcast_command("fleet-alpha"),
            "fleet/fleet-alpha/broadcast/command/request"
        );
        assert_eq!(
            broadcast_config("fleet-alpha"),
            "fleet/fleet-alpha/broadcast/config/update"
        );
    }

    #[test]
    fn wildcard_subscriptions() {
        assert_eq!(
            device_subscribe_all("fleet-alpha", "rpi-001"),
            "fleet/fleet-alpha/rpi-001/#"
        );
        assert_eq!(
            fleet_command_responses("fleet-alpha"),
            "fleet/fleet-alpha/+/command/response"
        );
        assert_eq!(
            fleet_heartbeats("fleet-alpha"),
            "fleet/fleet-alpha/+/heartbeat/ping"
        );
    }

    #[test]
    fn parse_device_topic() {
        let parsed = parse_topic("fleet/fleet-alpha/rpi-001/command/request").unwrap();
        assert_eq!(parsed.fleet_id, "fleet-alpha");
        assert_eq!(parsed.device_id, Some("rpi-001".into()));
        assert_eq!(parsed.category, "command");
        assert_eq!(parsed.action, "request");
    }

    #[test]
    fn parse_broadcast_topic() {
        let parsed = parse_topic("fleet/fleet-alpha/broadcast/command/request").unwrap();
        assert_eq!(parsed.fleet_id, "fleet-alpha");
        assert_eq!(parsed.device_id, None);
        assert_eq!(parsed.category, "command");
        assert_eq!(parsed.action, "request");
    }

    #[test]
    fn fleet_shadow_updates_topic() {
        assert_eq!(
            fleet_shadow_updates("fleet-alpha"),
            "fleet/fleet-alpha/+/shadow/update"
        );
    }

    #[test]
    fn parse_invalid_topic() {
        assert!(parse_topic("invalid/topic").is_none());
        assert!(parse_topic("fleet/abc").is_none());
        assert!(parse_topic("").is_none());
    }
}
