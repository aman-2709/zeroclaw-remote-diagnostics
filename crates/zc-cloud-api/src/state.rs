//! Shared application state for the Axum server.
//!
//! Supports two modes:
//! - **Database mode**: uses `PgPool` for persistent storage (production).
//! - **In-memory mode**: uses `RwLock<HashMap>` (tests and development).

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::{RwLock, broadcast};
use uuid::Uuid;

use zc_protocol::commands::{CommandEnvelope, CommandResponse};
use zc_protocol::device::{DeviceInfo, DeviceStatus, HardwareType};

use crate::events::WsEvent;
use crate::inference::InferenceEngine;

/// Shared application state, wrapped in `Arc` for Axum handler sharing.
#[derive(Clone)]
pub struct AppState {
    /// PostgreSQL connection pool (None in test/in-memory mode).
    pub pool: Option<PgPool>,
    /// In-memory device registry (used when pool is None).
    pub devices: Arc<RwLock<HashMap<String, DeviceInfo>>>,
    /// In-memory command log (used when pool is None).
    pub commands: Arc<RwLock<Vec<CommandRecord>>>,
    /// Broadcast channel for real-time WebSocket events.
    pub event_tx: broadcast::Sender<WsEvent>,
    /// NL inference engine for command parsing.
    pub inference: Arc<dyn InferenceEngine>,
}

/// A command with its response (if available).
#[derive(Debug, Clone)]
pub struct CommandRecord {
    pub envelope: CommandEnvelope,
    pub response: Option<CommandResponse>,
    pub created_at: DateTime<Utc>,
}

impl AppState {
    /// Create state backed by a PostgreSQL pool.
    pub fn with_pool(pool: PgPool) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            pool: Some(pool),
            devices: Arc::new(RwLock::new(HashMap::new())),
            commands: Arc::new(RwLock::new(Vec::new())),
            event_tx,
            inference: Arc::new(crate::inference::RuleBasedEngine::new()),
        }
    }

    /// Create in-memory state (for tests).
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            pool: None,
            devices: Arc::new(RwLock::new(HashMap::new())),
            commands: Arc::new(RwLock::new(Vec::new())),
            event_tx,
            inference: Arc::new(crate::inference::RuleBasedEngine::new()),
        }
    }

    /// Create state with sample devices for development / tests.
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

        let (event_tx, _) = broadcast::channel(256);
        Self {
            pool: None,
            devices: Arc::new(RwLock::new(devices)),
            commands: Arc::new(RwLock::new(Vec::new())),
            event_tx,
            inference: Arc::new(crate::inference::RuleBasedEngine::new()),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
