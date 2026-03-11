//! UDS service allowlist — enforces read-only diagnostics.
//!
//! Only allows diagnostic read operations. All write, flash, and security
//! services are blocked to prevent accidental ECU damage.

/// UDS services allowed in read-only PoC mode.
pub const ALLOWED_UDS_SERVICES: &[u8] = &[
    0x10, // DiagnosticSessionControl (session changes — read-only)
    0x19, // ReadDTCInformation
    0x22, // ReadDataByIdentifier
    0x3E, // TesterPresent (keep-alive)
];

/// UDS session types allowed (subset of DiagnosticSessionControl).
/// Programming session (0x02) is blocked — only needed for flashing.
pub const ALLOWED_SESSION_TYPES: &[u8] = &[
    0x01, // Default session
    0x03, // Extended diagnostic session
];

/// Validates that a UDS service ID is allowed under the current safety policy.
pub fn is_uds_service_allowed(service_id: u8) -> bool {
    ALLOWED_UDS_SERVICES.contains(&service_id)
}

/// Validates that a UDS session type is allowed.
pub fn is_session_type_allowed(session_type: u8) -> bool {
    ALLOWED_SESSION_TYPES.contains(&session_type)
}

/// Human-readable name for a UDS service ID.
pub fn uds_service_name(service_id: u8) -> &'static str {
    match service_id {
        0x10 => "DiagnosticSessionControl",
        0x11 => "ECUReset",
        0x14 => "ClearDiagnosticInformation",
        0x19 => "ReadDTCInformation",
        0x22 => "ReadDataByIdentifier",
        0x27 => "SecurityAccess",
        0x28 => "CommunicationControl",
        0x2E => "WriteDataByIdentifier",
        0x31 => "RoutineControl",
        0x34 => "RequestDownload",
        0x35 => "RequestUpload",
        0x36 => "TransferData",
        0x37 => "RequestTransferExit",
        0x3E => "TesterPresent",
        0x85 => "ControlDTCSetting",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_services() {
        assert!(is_uds_service_allowed(0x10)); // DiagnosticSessionControl
        assert!(is_uds_service_allowed(0x19)); // ReadDTCInformation
        assert!(is_uds_service_allowed(0x22)); // ReadDataByIdentifier
        assert!(is_uds_service_allowed(0x3E)); // TesterPresent
    }

    #[test]
    fn blocked_write_services() {
        assert!(!is_uds_service_allowed(0x14)); // ClearDTC
        assert!(!is_uds_service_allowed(0x27)); // SecurityAccess
        assert!(!is_uds_service_allowed(0x2E)); // WriteDataByIdentifier
        assert!(!is_uds_service_allowed(0x31)); // RoutineControl
    }

    #[test]
    fn blocked_flash_services() {
        assert!(!is_uds_service_allowed(0x34)); // RequestDownload
        assert!(!is_uds_service_allowed(0x35)); // RequestUpload
        assert!(!is_uds_service_allowed(0x36)); // TransferData
        assert!(!is_uds_service_allowed(0x37)); // RequestTransferExit
    }

    #[test]
    fn blocked_destructive_services() {
        assert!(!is_uds_service_allowed(0x11)); // ECUReset
        assert!(!is_uds_service_allowed(0x28)); // CommunicationControl
        assert!(!is_uds_service_allowed(0x85)); // ControlDTCSetting
    }

    #[test]
    fn allowed_session_types() {
        assert!(is_session_type_allowed(0x01)); // Default
        assert!(is_session_type_allowed(0x03)); // Extended
    }

    #[test]
    fn blocked_programming_session() {
        assert!(!is_session_type_allowed(0x02)); // Programming — blocked
    }

    #[test]
    fn service_names() {
        assert_eq!(uds_service_name(0x22), "ReadDataByIdentifier");
        assert_eq!(uds_service_name(0x19), "ReadDTCInformation");
        assert_eq!(uds_service_name(0x27), "SecurityAccess");
        assert_eq!(uds_service_name(0xFF), "Unknown");
    }
}
