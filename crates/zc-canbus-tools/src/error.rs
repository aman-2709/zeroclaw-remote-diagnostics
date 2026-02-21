//! CAN bus error types.

use thiserror::Error;

/// Errors that can occur during CAN bus operations.
#[derive(Debug, Error)]
pub enum CanError {
    #[error("CAN interface error: {0}")]
    Interface(String),

    #[error("OBD-II protocol error: {0}")]
    Protocol(String),

    #[error("Safety violation: mode 0x{mode:02X} is not allowed")]
    SafetyViolation { mode: u8 },

    #[error("Response timeout after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    #[error("ISO-TP reassembly error: {0}")]
    IsoTp(String),

    #[error("PID decode error: unknown PID 0x{pid:02X}")]
    UnknownPid { pid: u8 },

    #[error("Frame decode error: {0}")]
    Decode(String),

    #[error("{0}")]
    Other(String),
}

/// Convenience alias for CAN bus results.
pub type CanResult<T> = Result<T, CanError>;
