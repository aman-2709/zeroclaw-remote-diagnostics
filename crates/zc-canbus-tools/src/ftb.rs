//! UDS Failure Type Byte (FTB) decoder.
//!
//! The third byte of a UDS DTC encodes the failure type per ISO 14229-1 Annex D.
//! This module decodes that byte into a human-readable description.

/// Decode a UDS Failure Type Byte into a human-readable description.
///
/// Based on ISO 14229-1 Annex D.2 (DTC failure type definitions).
pub fn decode_ftb(byte: u8) -> &'static str {
    match byte {
        0x00 => "No Failure Type Information",
        // 0x01–0x0F: General electrical failures
        0x01 => "General Electrical Failure",
        0x02 => "General Signal Failure",
        0x03 => "No Signal",
        0x04 => "Intermittent Signal",
        0x05 => "Invalid Signal",
        // 0x06–0x0F: Circuit failures
        0x06 => "Circuit Open",
        0x07 => "Circuit Short to Ground",
        0x08 => "Circuit Short to Battery",
        0x09 => "Circuit Open or Short to Ground",
        0x0A => "Circuit Open or Short to Battery",
        0x0B => "Circuit Short to Ground or Battery",
        0x0C => "Circuit Open or Short to Ground or Battery",
        // 0x0D–0x0F: Reserved
        // 0x10–0x1F: Signal range/performance
        0x11 => "Circuit Short to Ground",
        0x12 => "Circuit Short to Battery",
        0x13 => "Circuit Open",
        0x14 => "Circuit Active",
        0x15 => "Circuit Passive",
        0x16 => "Circuit/Component Current Above Threshold",
        0x17 => "Circuit/Component Current Below Threshold",
        0x18 => "Circuit/Component Voltage Above Threshold",
        0x19 => "Circuit/Component Voltage Below Threshold",
        0x1A => "Circuit/Component Resistance Above Threshold",
        0x1B => "Circuit/Component Resistance Below Threshold",
        0x1C => "Circuit/Component Temperature Above Threshold",
        0x1D => "Circuit/Component Temperature Below Threshold",
        // 0x20–0x2F: Signal characteristics
        0x21 => "Signal Range Check Failure High",
        0x22 => "Signal Range Check Failure Low",
        0x23 => "Signal Stuck High",
        0x24 => "Signal Stuck Low",
        0x25 => "No Signal (Missing Message)",
        0x26 => "Signal Invalid",
        0x27 => "Signal Rate of Change Above Threshold",
        0x28 => "Signal Rate of Change Below Threshold",
        0x29 => "Signal Erratic",
        // 0x30–0x3F: System/component performance
        0x31 => "No Sub-Type Information",
        // 0x40–0x4F: Component/system functionality
        0x41 => "General Checksum Failure",
        0x42 => "General Checksum Failure",
        0x44 => "Data Memory Failure",
        0x45 => "Program Memory Failure",
        0x46 => "Calibration/Parameter Memory Failure",
        // 0x50–0x5F: Plausibility
        0x51 => "Not Activated",
        0x52 => "Activated",
        // 0x60–0x6F: Timing
        0x61 => "General Performance",
        0x62 => "Performance Too Slow",
        0x63 => "Performance Too Fast",
        0x64 => "Stuck",
        0x65 => "No Operation",
        // 0x70–0x7F: Component
        0x71 => "Component Obstructed/Blocked",
        0x72 => "Locked",
        0x73 => "Component Adjustment/Alignment",
        // 0x80–0x8F: Bus/message failures
        0x81 => "Bus Signal/Message Failures",
        0x82 => "Bus Signal/Message Not Received",
        // 0x90–0x9F: Component internal
        0x91 => "Component Internal Failure",
        0x92 => "Component Internal Stuck",
        0x93 => "Component Internal Incorrect",
        // 0xFE–0xFF
        0xFE => "Sub-Type Not Yet Determined",
        0xFF => "No Failure Type Information",
        _ => "Reserved",
    }
}

/// Format a FTB byte as a description string suitable for display.
///
/// For known values, returns the description.
/// For unknown/reserved values, includes the hex byte.
pub fn format_ftb(byte: u8) -> String {
    let desc = decode_ftb(byte);
    if desc == "Reserved" {
        format!("Reserved (0x{byte:02X})")
    } else {
        desc.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_electrical_failures() {
        assert_eq!(decode_ftb(0x06), "Circuit Open");
        assert_eq!(decode_ftb(0x07), "Circuit Short to Ground");
        assert_eq!(decode_ftb(0x08), "Circuit Short to Battery");
    }

    #[test]
    fn known_signal_range() {
        assert_eq!(decode_ftb(0x21), "Signal Range Check Failure High");
        assert_eq!(decode_ftb(0x22), "Signal Range Check Failure Low");
        assert_eq!(decode_ftb(0x29), "Signal Erratic");
    }

    #[test]
    fn known_checksum() {
        assert_eq!(decode_ftb(0x41), "General Checksum Failure");
        assert_eq!(decode_ftb(0x42), "General Checksum Failure");
    }

    #[test]
    fn no_failure_type() {
        assert_eq!(decode_ftb(0x00), "No Failure Type Information");
        assert_eq!(decode_ftb(0xFF), "No Failure Type Information");
    }

    #[test]
    fn reserved_byte() {
        assert_eq!(decode_ftb(0xAA), "Reserved");
        assert_eq!(decode_ftb(0x30), "Reserved");
    }

    #[test]
    fn format_known_value() {
        assert_eq!(format_ftb(0x07), "Circuit Short to Ground");
    }

    #[test]
    fn format_reserved_includes_hex() {
        assert_eq!(format_ftb(0xAA), "Reserved (0xAA)");
        assert_eq!(format_ftb(0x30), "Reserved (0x30)");
    }

    #[test]
    fn component_and_timing() {
        assert_eq!(decode_ftb(0x61), "General Performance");
        assert_eq!(decode_ftb(0x64), "Stuck");
        assert_eq!(decode_ftb(0x71), "Component Obstructed/Blocked");
    }

    #[test]
    fn bus_and_internal() {
        assert_eq!(decode_ftb(0x81), "Bus Signal/Message Failures");
        assert_eq!(decode_ftb(0x91), "Component Internal Failure");
    }
}
