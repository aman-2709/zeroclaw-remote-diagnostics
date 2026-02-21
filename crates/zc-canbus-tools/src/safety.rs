//! CAN bus safety guard — enforces read-only mode and PID whitelisting.
//!
//! Allowed OBD-II modes (PoC):
//! - 0x01: Show current data (live PIDs)
//! - 0x02: Show freeze frame data
//! - 0x03: Show stored DTCs
//! - 0x09: Request vehicle information (VIN)
//!
//! All write operations (Mode 0x04 clear DTCs, etc.) are blocked.

/// OBD-II modes allowed in read-only PoC mode.
pub const ALLOWED_MODES: &[u8] = &[0x01, 0x02, 0x03, 0x09];

/// Validates that an OBD-II mode is allowed under the current safety policy.
pub fn is_mode_allowed(mode: u8) -> bool {
    ALLOWED_MODES.contains(&mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_modes() {
        assert!(is_mode_allowed(0x01)); // Current data
        assert!(is_mode_allowed(0x02)); // Freeze frame
        assert!(is_mode_allowed(0x03)); // Stored DTCs
        assert!(is_mode_allowed(0x09)); // Vehicle info
    }

    #[test]
    fn blocked_modes() {
        assert!(!is_mode_allowed(0x04)); // Clear DTCs — WRITE
        assert!(!is_mode_allowed(0x05)); // O2 sensor test
        assert!(!is_mode_allowed(0x07)); // Pending DTCs
        assert!(!is_mode_allowed(0x08)); // Control on-board — WRITE
        assert!(!is_mode_allowed(0x0A)); // Permanent DTCs
    }
}
