//! CAN bus / OBD-II diagnostic tools for ZeroClaw.
//!
//! Provides a trait-based CAN interface abstraction, OBD-II protocol helpers,
//! ISO-TP multi-frame support, a static DTC database, and 5 diagnostic tools.

pub mod dtc_db;
pub mod error;
pub mod interface;
pub mod mock;
pub mod obd;
pub mod safety;
pub mod tools;
pub mod types;

// Re-export key types for convenience
pub use error::{CanError, CanResult};
pub use interface::CanInterface;
pub use mock::MockCanInterface;
pub use types::{CanFrame, CanTool, ToolResult};
