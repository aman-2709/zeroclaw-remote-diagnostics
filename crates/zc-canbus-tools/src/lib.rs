//! CAN bus / OBD-II / UDS diagnostic tools for ZeroClaw.
//!
//! Provides a trait-based CAN interface abstraction, OBD-II protocol helpers,
//! UDS (ISO 14229) protocol support for Hella ECUs, ISO-TP multi-frame support,
//! a static DTC database, and 8 diagnostic tools.

pub mod dtc_db;
pub mod ecu_profile;
pub mod error;
pub mod ftb;
pub mod interface;
pub mod mock;
pub mod obd;
pub mod safety;
pub mod tools;
pub mod types;
pub mod uds;
pub mod uds_safety;

// Re-export key types for convenience
pub use error::{CanError, CanResult};
pub use interface::CanInterface;
#[cfg(target_os = "linux")]
pub use interface::SocketCanInterface;
pub use mock::MockCanInterface;
pub use types::{CanFrame, CanTool, ToolResult};
