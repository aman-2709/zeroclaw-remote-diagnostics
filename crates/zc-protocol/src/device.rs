use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique fleet identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FleetId(pub Uuid);

impl FleetId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for FleetId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for FleetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Device lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceStatus {
    Provisioning,
    Online,
    Offline,
    Maintenance,
    Decommissioned,
}

/// Hardware type of the edge device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareType {
    RaspberryPi4,
    RaspberryPi5,
    IndustrialSbc,
    Custom(String),
}

/// Core device information stored in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Internal database ID.
    pub id: Uuid,
    /// Fleet this device belongs to.
    pub fleet_id: FleetId,
    /// IoT Core thing name (unique within fleet).
    pub device_id: String,
    /// Current lifecycle status.
    pub status: DeviceStatus,
    /// Vehicle Identification Number (if assigned).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vin: Option<String>,
    /// Hardware platform.
    pub hardware_type: HardwareType,
    /// X.509 certificate ID for mTLS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_id: Option<String>,
    /// Last heartbeat received from the device.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_heartbeat: Option<DateTime<Utc>>,
    /// Flexible metadata (firmware version, location, etc.).
    #[serde(default)]
    pub metadata: serde_json::Value,
    /// When the device was registered.
    pub created_at: DateTime<Utc>,
    /// Last updated timestamp.
    pub updated_at: DateTime<Utc>,
}

/// Heartbeat message sent by devices on a 30-second interval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heartbeat {
    pub device_id: String,
    pub fleet_id: String,
    pub status: DeviceStatus,
    pub uptime_secs: u64,
    pub ollama_status: ServiceStatus,
    pub can_status: ServiceStatus,
    pub agent_version: String,
    /// Stable hardware fingerprint from `/etc/machine-id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub machine_id: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Status of an edge subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStatus {
    Running,
    Stopped,
    Error,
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_status_serialization() {
        assert_eq!(
            serde_json::to_string(&DeviceStatus::Online).unwrap(),
            r#""online""#
        );
        assert_eq!(
            serde_json::to_string(&DeviceStatus::Decommissioned).unwrap(),
            r#""decommissioned""#
        );
    }

    #[test]
    fn hardware_type_variants() {
        let rpi4 = HardwareType::RaspberryPi4;
        let json = serde_json::to_string(&rpi4).unwrap();
        assert_eq!(json, r#""raspberry_pi4""#);

        let custom = HardwareType::Custom("BeagleBone".into());
        let json = serde_json::to_string(&custom).unwrap();
        assert!(json.contains("BeagleBone"));
    }

    #[test]
    fn fleet_id_display() {
        let fleet = FleetId::new();
        let display = format!("{fleet}");
        // UUIDv7 format: xxxxxxxx-xxxx-7xxx-xxxx-xxxxxxxxxxxx
        assert_eq!(display.len(), 36);
    }

    #[test]
    fn heartbeat_roundtrip() {
        let hb = Heartbeat {
            device_id: "rpi-001".into(),
            fleet_id: "fleet-alpha".into(),
            status: DeviceStatus::Online,
            uptime_secs: 3600,
            ollama_status: ServiceStatus::Running,
            can_status: ServiceStatus::Running,
            agent_version: "0.1.0".into(),
            machine_id: Some("a8b9c0d1e2f34567890abcdef0123456".into()),
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&hb).unwrap();
        let deserialized: Heartbeat = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.device_id, "rpi-001");
        assert_eq!(deserialized.ollama_status, ServiceStatus::Running);
        assert_eq!(
            deserialized.machine_id.as_deref(),
            Some("a8b9c0d1e2f34567890abcdef0123456")
        );
    }

    #[test]
    fn heartbeat_without_machine_id_deserializes() {
        // Backward compat: older agents won't send machine_id.
        let json = r#"{
            "device_id": "rpi-001",
            "fleet_id": "fleet-alpha",
            "status": "online",
            "uptime_secs": 100,
            "ollama_status": "running",
            "can_status": "stopped",
            "agent_version": "0.1.0",
            "timestamp": "2026-03-10T12:00:00Z"
        }"#;
        let hb: Heartbeat = serde_json::from_str(json).unwrap();
        assert!(hb.machine_id.is_none());
    }
}
