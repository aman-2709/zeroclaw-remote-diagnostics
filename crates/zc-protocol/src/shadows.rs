use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Device shadow state — a cached view of device-reported and cloud-desired state.
///
/// Modeled after AWS IoT Device Shadows: reported (from device),
/// desired (from cloud), and delta (difference).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowState {
    /// State reported by the device.
    #[serde(default)]
    pub reported: serde_json::Value,
    /// State desired by the cloud.
    #[serde(default)]
    pub desired: serde_json::Value,
    /// Shadow version (monotonically increasing).
    pub version: u64,
    /// Last updated timestamp.
    pub last_updated: DateTime<Utc>,
}

/// A named shadow for a device (AWS IoT supports multiple shadows per thing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedShadow {
    /// Shadow name (e.g., "diagnostics", "config", "telemetry-settings").
    pub name: String,
    /// Shadow state.
    pub state: ShadowState,
}

/// Delta message sent from cloud to device when desired state diverges from reported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowDelta {
    /// Device this delta targets.
    pub device_id: String,
    /// Shadow name.
    pub shadow_name: String,
    /// The delta (difference between desired and reported).
    pub delta: serde_json::Value,
    /// Shadow version this delta was computed from.
    pub version: u64,
    /// When the delta was generated.
    pub timestamp: DateTime<Utc>,
}

/// Shadow update request from the device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowUpdate {
    /// Device sending the update.
    pub device_id: String,
    /// Shadow name being updated.
    pub shadow_name: String,
    /// Reported state to merge.
    pub reported: serde_json::Value,
    /// Current device-side version.
    pub version: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn shadow_state_roundtrip() {
        let shadow = ShadowState {
            reported: json!({
                "firmware_version": "0.1.0",
                "ollama_model": "phi3:mini",
                "can_interface": "can0"
            }),
            desired: json!({
                "firmware_version": "0.2.0"
            }),
            version: 42,
            last_updated: Utc::now(),
        };
        let json = serde_json::to_string(&shadow).unwrap();
        let deserialized: ShadowState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.version, 42);
        assert_eq!(deserialized.reported["firmware_version"], "0.1.0");
    }

    #[test]
    fn shadow_delta_roundtrip() {
        let delta = ShadowDelta {
            device_id: "rpi-001".into(),
            shadow_name: "config".into(),
            delta: json!({
                "firmware_version": "0.2.0"
            }),
            version: 43,
            timestamp: Utc::now(),
        };
        let json_str = serde_json::to_string(&delta).unwrap();
        let deserialized: ShadowDelta = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.device_id, "rpi-001");
        assert_eq!(deserialized.delta["firmware_version"], "0.2.0");
    }

    #[test]
    fn named_shadow_roundtrip() {
        let shadow = NamedShadow {
            name: "diagnostics".into(),
            state: ShadowState {
                reported: json!({"dtc_count": 3}),
                desired: json!({}),
                version: 1,
                last_updated: Utc::now(),
            },
        };
        let json = serde_json::to_string(&shadow).unwrap();
        let deserialized: NamedShadow = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "diagnostics");
        assert_eq!(deserialized.state.reported["dtc_count"], 3);
    }
}
