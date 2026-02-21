//! Core CAN bus types, OBD-II constants, and the CanTool trait.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::CanResult;

// ── OBD-II CAN IDs ──────────────────────────────────────────────

/// Standard OBD-II broadcast request CAN ID.
pub const OBD_REQUEST_ID: u32 = 0x7DF;

/// First OBD-II response CAN ID (ECU #1).
pub const OBD_RESPONSE_ID_MIN: u32 = 0x7E8;

/// Last OBD-II response CAN ID (ECU #8).
pub const OBD_RESPONSE_ID_MAX: u32 = 0x7EF;

// ── OBD-II Modes ────────────────────────────────────────────────

/// Mode 01: Show current data (live PIDs).
pub const MODE_CURRENT_DATA: u8 = 0x01;

/// Mode 02: Show freeze frame data.
pub const MODE_FREEZE_FRAME: u8 = 0x02;

/// Mode 03: Show stored DTCs.
pub const MODE_STORED_DTCS: u8 = 0x03;

/// Mode 09: Request vehicle information (VIN, etc.).
pub const MODE_VEHICLE_INFO: u8 = 0x09;

/// Offset added to request mode to get response SID.
pub const RESPONSE_SID_OFFSET: u8 = 0x40;

// ── ISO-TP frame type nibbles (upper nibble of byte 0) ────────

/// Single Frame.
pub const ISOTP_SF: u8 = 0x0;
/// First Frame.
pub const ISOTP_FF: u8 = 0x1;
/// Consecutive Frame.
pub const ISOTP_CF: u8 = 0x2;
/// Flow Control.
pub const ISOTP_FC: u8 = 0x3;

// ── CAN Frame ───────────────────────────────────────────────────

/// A raw CAN 2.0A frame (standard 11-bit ID).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanFrame {
    /// CAN arbitration ID (11-bit standard).
    pub id: u32,
    /// Data payload (0–8 bytes for standard CAN).
    pub data: Vec<u8>,
}

impl CanFrame {
    pub fn new(id: u32, data: Vec<u8>) -> Self {
        Self { id, data }
    }
}

// ── Tool Result ─────────────────────────────────────────────────

/// Result of executing a CAN bus diagnostic tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Tool name that produced this result.
    pub tool_name: String,
    /// Whether the tool execution succeeded.
    pub success: bool,
    /// Structured result data (JSON).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// Human-readable summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Error message if success is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ToolResult {
    pub fn success(
        tool_name: impl Into<String>,
        data: serde_json::Value,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            tool_name: tool_name.into(),
            success: true,
            data: Some(data),
            summary: Some(summary.into()),
            error: None,
        }
    }

    pub fn failure(tool_name: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            success: false,
            data: None,
            summary: None,
            error: Some(error.into()),
        }
    }
}

// ── CanTool Trait ────────────────────────────────────────────────

/// Trait for CAN bus diagnostic tools.
///
/// Structurally identical to ZeroClaw's `Tool` trait but owned by this crate.
/// Trivially wrappable via thin adapter when wiring into the fleet agent.
#[async_trait]
pub trait CanTool: Send + Sync {
    /// Tool name (e.g., "read_dtcs").
    fn name(&self) -> &str;

    /// Human-readable description.
    fn description(&self) -> &str;

    /// JSON Schema describing accepted arguments.
    fn parameters_schema(&self) -> serde_json::Value;

    /// Execute the tool with JSON arguments against a CAN interface.
    async fn execute(
        &self,
        args: serde_json::Value,
        interface: &dyn crate::interface::CanInterface,
    ) -> CanResult<ToolResult>;
}
