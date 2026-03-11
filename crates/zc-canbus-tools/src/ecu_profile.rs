//! ECU profile definitions for UDS-capable ECUs.
//!
//! Each profile specifies CAN IDs, bitrate, known DIDs, and decoding rules
//! for a specific ECU type. Profiles are statically defined and looked up
//! by name at runtime.

/// An ECU's communication profile for UDS diagnostics.
#[derive(Debug, Clone)]
pub struct EcuProfile {
    /// Short name: "BCR", "BCF", "TM".
    pub name: &'static str,
    /// UDS request CAN ID (e.g., 0x60D for BCR).
    pub request_id: u32,
    /// UDS response CAN ID (e.g., 0x58D for BCR).
    pub response_id: u32,
    /// CAN bus bitrate in kbps.
    pub bitrate_kbps: u16,
    /// CAN interface name (e.g., "can0").
    pub can_interface: &'static str,
    /// Known DIDs for this ECU.
    pub known_dids: &'static [DidEntry],
}

/// A single Data Identifier (DID) known to live on an ECU.
#[derive(Debug, Clone)]
pub struct DidEntry {
    /// UDS DID number (e.g., 0xFD05).
    pub did: u16,
    /// Human-readable name.
    pub name: &'static str,
    /// Application-level diagnostic code (e.g., 3005).
    pub diag_code: u16,
    /// How to decode the response bytes.
    pub decode: DidDecoder,
}

/// Decoding strategy for a DID's response payload.
#[derive(Debug, Clone, Copy)]
pub enum DidDecoder {
    /// Decode as float: `value = raw * scale + offset`, with a unit string.
    Float {
        scale: f64,
        offset: f64,
        unit: &'static str,
    },
    /// Decode as boolean (0x00 = false, anything else = true).
    Bool,
    /// Decode as unsigned 16-bit integer (big-endian).
    U16,
    /// Return raw bytes as hex string.
    Bytes,
}

/// Decode raw response bytes according to a `DidDecoder`.
///
/// Returns a `serde_json::Value` representing the decoded value.
pub fn decode_did_value(decoder: &DidDecoder, data: &[u8]) -> serde_json::Value {
    match decoder {
        DidDecoder::Float {
            scale,
            offset,
            unit,
        } => {
            let raw = match data.len() {
                0 => return serde_json::json!({"error": "no data"}),
                1 => data[0] as f64,
                _ => (data[0] as f64) * 256.0 + data[1] as f64,
            };
            let value = raw * scale + offset;
            serde_json::json!({
                "value": (value * 100.0).round() / 100.0,
                "unit": unit,
            })
        }
        DidDecoder::Bool => {
            let value = data.first().map(|&b| b != 0).unwrap_or(false);
            serde_json::json!({"value": value})
        }
        DidDecoder::U16 => {
            let value = match data.len() {
                0 => 0u16,
                1 => data[0] as u16,
                _ => ((data[0] as u16) << 8) | data[1] as u16,
            };
            serde_json::json!({"value": value})
        }
        DidDecoder::Bytes => {
            let hex: String = data
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            serde_json::json!({"value": hex})
        }
    }
}

// ── Hella BCR (Body Control Rear) ────────────────────────────────

// PLACEHOLDER — replace with actual DID from esync_hella_diag_codes.h
const BCR_DIDS: &[DidEntry] = &[
    DidEntry {
        did: 0xFD05, // PLACEHOLDER
        name: "Power Supply Voltage",
        diag_code: 3005,
        decode: DidDecoder::Float {
            scale: 0.01,
            offset: 0.0,
            unit: "V",
        },
    },
    DidEntry {
        did: 0xFD10, // PLACEHOLDER
        name: "Open Load DTC — Brake Light LEFT",
        diag_code: 3010,
        decode: DidDecoder::Bool,
    },
    DidEntry {
        did: 0xFD11, // PLACEHOLDER
        name: "Open Load DTC — Brake Light RIGHT",
        diag_code: 3011,
        decode: DidDecoder::Bool,
    },
    DidEntry {
        did: 0xFD50, // PLACEHOLDER
        name: "Reprogramming Attempts Used",
        diag_code: 3050,
        decode: DidDecoder::U16,
    },
];

/// Hella BCR ECU profile.
pub static HELLA_BCR: EcuProfile = EcuProfile {
    name: "BCR",
    request_id: 0x60D,
    response_id: 0x58D,
    bitrate_kbps: 250,
    can_interface: "can0",
    known_dids: BCR_DIDS,
};

// ── Hella BCF (Body Control Front) ──────────────────────────────

// PLACEHOLDER — replace with actual DID from esync_hella_diag_codes.h
const BCF_DIDS: &[DidEntry] = &[
    DidEntry {
        did: 0xFD05, // PLACEHOLDER
        name: "Power Supply Voltage",
        diag_code: 3005,
        decode: DidDecoder::Float {
            scale: 0.01,
            offset: 0.0,
            unit: "V",
        },
    },
    DidEntry {
        did: 0xFD50, // PLACEHOLDER
        name: "Reprogramming Attempts Used",
        diag_code: 3050,
        decode: DidDecoder::U16,
    },
];

/// Hella BCF ECU profile.
pub static HELLA_BCF: EcuProfile = EcuProfile {
    name: "BCF",
    request_id: 0x609,
    response_id: 0x589,
    bitrate_kbps: 250,
    can_interface: "can0",
    known_dids: BCF_DIDS,
};

// ── Profile registry ─────────────────────────────────────────────

/// All known ECU profiles.
pub fn all_profiles() -> Vec<&'static EcuProfile> {
    vec![&HELLA_BCR, &HELLA_BCF]
}

/// Look up an ECU profile by name (case-insensitive).
pub fn find_profile(name: &str) -> Option<&'static EcuProfile> {
    let upper = name.to_uppercase();
    all_profiles().into_iter().find(|p| p.name == upper)
}

/// Look up a DID entry within a profile by DID number.
pub fn find_did(profile: &EcuProfile, did: u16) -> Option<&DidEntry> {
    profile.known_dids.iter().find(|d| d.did == did)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_bcr_profile() {
        let p = find_profile("BCR").unwrap();
        assert_eq!(p.request_id, 0x60D);
        assert_eq!(p.response_id, 0x58D);
        assert_eq!(p.bitrate_kbps, 250);
    }

    #[test]
    fn find_bcf_profile() {
        let p = find_profile("BCF").unwrap();
        assert_eq!(p.request_id, 0x609);
        assert_eq!(p.response_id, 0x589);
    }

    #[test]
    fn find_profile_case_insensitive() {
        assert!(find_profile("bcr").is_some());
        assert!(find_profile("Bcf").is_some());
    }

    #[test]
    fn find_profile_unknown() {
        assert!(find_profile("TM").is_none());
        assert!(find_profile("").is_none());
    }

    #[test]
    fn all_profiles_count() {
        assert_eq!(all_profiles().len(), 2);
    }

    #[test]
    fn bcr_has_four_dids() {
        assert_eq!(HELLA_BCR.known_dids.len(), 4);
    }

    #[test]
    fn bcf_has_two_dids() {
        assert_eq!(HELLA_BCF.known_dids.len(), 2);
    }

    #[test]
    fn find_did_voltage() {
        let entry = find_did(&HELLA_BCR, 0xFD05).unwrap();
        assert_eq!(entry.name, "Power Supply Voltage");
        assert_eq!(entry.diag_code, 3005);
    }

    #[test]
    fn find_did_unknown() {
        assert!(find_did(&HELLA_BCR, 0x0000).is_none());
    }

    // ── DidDecoder tests ─────────────────────────────────────────

    #[test]
    fn decode_float_voltage() {
        let dec = DidDecoder::Float {
            scale: 0.01,
            offset: 0.0,
            unit: "V",
        };
        // 0x04, 0xCA → 1226 → 1226 * 0.01 = 12.26 V
        let result = decode_did_value(&dec, &[0x04, 0xCA]);
        assert_eq!(result["unit"], "V");
        let v = result["value"].as_f64().unwrap();
        assert!((v - 12.26).abs() < 0.01);
    }

    #[test]
    fn decode_bool_true() {
        let result = decode_did_value(&DidDecoder::Bool, &[0x01]);
        assert_eq!(result["value"], true);
    }

    #[test]
    fn decode_bool_false() {
        let result = decode_did_value(&DidDecoder::Bool, &[0x00]);
        assert_eq!(result["value"], false);
    }

    #[test]
    fn decode_u16() {
        let result = decode_did_value(&DidDecoder::U16, &[0x00, 0x2A]);
        assert_eq!(result["value"], 42);
    }

    #[test]
    fn decode_bytes() {
        let result = decode_did_value(&DidDecoder::Bytes, &[0xDE, 0xAD]);
        assert_eq!(result["value"], "DE AD");
    }

    #[test]
    fn decode_float_empty_data() {
        let dec = DidDecoder::Float {
            scale: 1.0,
            offset: 0.0,
            unit: "V",
        };
        let result = decode_did_value(&dec, &[]);
        assert!(result.get("error").is_some());
    }

    #[test]
    fn decode_bool_empty_data() {
        let result = decode_did_value(&DidDecoder::Bool, &[]);
        assert_eq!(result["value"], false);
    }

    #[test]
    fn decode_u16_single_byte() {
        let result = decode_did_value(&DidDecoder::U16, &[0x05]);
        assert_eq!(result["value"], 5);
    }
}
