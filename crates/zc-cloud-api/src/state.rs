//! Shared application state for the Axum server.
//!
//! Phase 1 uses in-memory stores. Phase 2 replaces with PostgreSQL.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use uuid::Uuid;

use zc_protocol::commands::{CommandEnvelope, CommandResponse};
use zc_protocol::device::{DeviceInfo, DeviceStatus, HardwareType};

/// Shared application state, wrapped in `Arc` for Axum handler sharing.
#[derive(Clone)]
pub struct AppState {
    /// In-memory device registry (Phase 2: PostgreSQL).
    pub devices: Arc<RwLock<HashMap<String, DeviceInfo>>>,
    /// In-memory command log (Phase 2: PostgreSQL).
    pub commands: Arc<RwLock<Vec<CommandRecord>>>,
}

/// A command with its response (if available).
#[derive(Debug, Clone)]
pub struct CommandRecord {
    pub envelope: CommandEnvelope,
    pub response: Option<CommandResponse>,
    pub created_at: DateTime<Utc>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            devices: Arc::new(RwLock::new(HashMap::new())),
            commands: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create state with sample devices for development.
    pub fn with_sample_data() -> Self {
        let mut devices = HashMap::new();

        let now = Utc::now();
        for (id, fleet) in [
            ("rpi-001", "fleet-alpha"),
            ("rpi-002", "fleet-alpha"),
            ("sbc-010", "fleet-beta"),
        ] {
            devices.insert(
                id.to_string(),
                DeviceInfo {
                    id: Uuid::now_v7(),
                    fleet_id: zc_protocol::device::FleetId(Uuid::now_v7()),
                    device_id: id.to_string(),
                    status: DeviceStatus::Online,
                    vin: None,
                    hardware_type: HardwareType::RaspberryPi4,
                    certificate_id: None,
                    last_heartbeat: Some(now),
                    metadata: serde_json::json!({"fleet": fleet}),
                    created_at: now,
                    updated_at: now,
                },
            );
        }

        Self {
            devices: Arc::new(RwLock::new(devices)),
            commands: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
