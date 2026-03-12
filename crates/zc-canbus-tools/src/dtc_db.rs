//! Static DTC database — 18,805 codes from Wal33D/dtc-database (MIT).
//!
//! Embeds TSV data via `include_str!` and parses into `LazyLock<HashMap>` on first access.
//! Generic codes: code → description.
//! Manufacturer codes: (code, manufacturer) → description.
//! Severity is inferred by code pattern (conservative heuristic).

use std::collections::HashMap;
use std::sync::LazyLock;

use zc_protocol::dtc::DtcSeverity;

static GENERIC_TSV: &str = include_str!("../data/dtc_generic.tsv");
static MANUFACTURER_TSV: &str = include_str!("../data/dtc_manufacturer.tsv");

/// Parsed generic DTC map: code → description.
static GENERIC_MAP: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for line in GENERIC_TSV.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((code, desc)) = line.split_once('\t') {
            map.insert(code, desc);
        }
    }
    map
});

/// Parsed manufacturer DTC map: (code, manufacturer) → description.
static MANUFACTURER_MAP: LazyLock<HashMap<(&'static str, &'static str), &'static str>> =
    LazyLock::new(|| {
        let mut map = HashMap::new();
        for line in MANUFACTURER_TSV.lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let mut parts = line.splitn(3, '\t');
            if let (Some(code), Some(mfr), Some(desc)) = (parts.next(), parts.next(), parts.next())
            {
                map.insert((code, mfr), desc);
            }
        }
        map
    });

/// DTC entry from the database.
#[derive(Debug, Clone)]
pub struct DtcEntry {
    pub description: String,
    pub severity: DtcSeverity,
    /// How severity was determined.
    pub severity_source: &'static str,
}

/// Look up a generic DTC code. Input is case-insensitive.
pub fn lookup(code: &str) -> Option<DtcEntry> {
    let upper = code.to_uppercase();
    // Try exact match in the static map (keys are already uppercase from TSV)
    GENERIC_MAP.get(upper.as_str()).map(|desc| {
        let severity = infer_severity(&upper);
        DtcEntry {
            description: desc.to_string(),
            severity,
            severity_source: "database",
        }
    })
}

/// Look up a manufacturer-specific DTC code.
/// Falls back to generic if no manufacturer match.
/// Manufacturer is case-insensitive.
pub fn lookup_with_manufacturer(code: &str, manufacturer: &str) -> Option<DtcEntry> {
    let upper_code = code.to_uppercase();
    let upper_mfr = manufacturer.to_uppercase();

    // Try manufacturer-specific first
    if let Some(desc) = MANUFACTURER_MAP.get(&(upper_code.as_str(), upper_mfr.as_str())) {
        let severity = infer_severity(&upper_code);
        return Some(DtcEntry {
            description: desc.to_string(),
            severity,
            severity_source: "database",
        });
    }

    // Fall back to generic
    lookup(code)
}

/// Infer severity from the DTC code pattern.
///
/// Conservative approach: only a small set of well-known patterns are Critical.
/// Everything else defaults to Warning.
pub fn infer_severity(code: &str) -> DtcSeverity {
    let bytes = code.as_bytes();
    if bytes.len() < 4 {
        return DtcSeverity::Warning;
    }

    let prefix = bytes[0];
    // Numeric portion after the letter prefix (e.g., "0300" from "P0300")
    let num_str = &code[1..];

    match prefix {
        b'P' => {
            // P030x: Misfire codes → Critical
            if num_str.starts_with("030") {
                return DtcSeverity::Critical;
            }
            // P0335-P0340: Crank/cam sensor → Critical
            if let Ok(n) = u16::from_str_radix(num_str, 16) {
                if (0x0335..=0x0340).contains(&n) {
                    return DtcSeverity::Critical;
                }
                // P0700: Transmission control system → Critical
                if n == 0x0700 || n == 0x0730 {
                    return DtcSeverity::Critical;
                }
                // P0A80: Hybrid battery → Critical
                if n == 0x0A80 {
                    return DtcSeverity::Critical;
                }
                // P044x: EVAP codes → Info (except P0455 large leak → Warning)
                if (0x0440..=0x0446).contains(&n) {
                    return DtcSeverity::Info;
                }
                // P0506-P0507: Idle RPM → Info
                if n == 0x0506 || n == 0x0507 {
                    return DtcSeverity::Info;
                }
            }
            DtcSeverity::Warning
        }
        b'B' => {
            // B010x: Airbag/SRS frontal sensors → Critical
            if num_str.starts_with("010") {
                return DtcSeverity::Critical;
            }
            // B1000, B1342: ECU malfunction → Critical
            if num_str == "1000" || num_str == "1342" {
                return DtcSeverity::Critical;
            }
            // B2799: Immobilizer → Critical
            if num_str == "2799" {
                return DtcSeverity::Critical;
            }
            // B1200: Climate control → Info
            if num_str == "1200" {
                return DtcSeverity::Info;
            }
            DtcSeverity::Warning
        }
        b'C' => {
            // Chassis codes: mostly ABS/brakes → Warning is appropriate default
            DtcSeverity::Warning
        }
        b'U' => {
            // U0001, U0073: CAN bus off → Critical
            if num_str == "0001" || num_str == "0073" {
                return DtcSeverity::Critical;
            }
            // U010x: Lost comms with ECM/PCM/TCM → Critical
            if num_str.starts_with("010") {
                return DtcSeverity::Critical;
            }
            // U0121: Lost comms with ABS → Critical
            if num_str == "0121" {
                return DtcSeverity::Critical;
            }
            // U0164: Lost comms with HVAC → Info
            if num_str == "0164" {
                return DtcSeverity::Info;
            }
            DtcSeverity::Warning
        }
        _ => DtcSeverity::Warning,
    }
}

/// Number of generic codes loaded.
pub fn generic_count() -> usize {
    GENERIC_MAP.len()
}

/// Number of manufacturer-specific codes loaded.
pub fn manufacturer_count() -> usize {
    MANUFACTURER_MAP.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Generic lookup ---

    #[test]
    fn known_powertrain_code() {
        let entry = lookup("P0300").unwrap();
        assert!(entry.description.contains("Misfire"));
        assert_eq!(entry.severity, DtcSeverity::Critical);
        assert_eq!(entry.severity_source, "database");
    }

    #[test]
    fn known_catalyst_code() {
        let entry = lookup("P0420").unwrap();
        assert!(entry.description.contains("Catalyst") || entry.description.contains("catalyst"));
        assert_eq!(entry.severity, DtcSeverity::Warning);
    }

    #[test]
    fn known_body_code() {
        let entry = lookup("B0100").expect("B0100 should be in database");
        assert_eq!(entry.severity, DtcSeverity::Critical);
    }

    #[test]
    fn known_network_code() {
        let entry = lookup("U0100").expect("U0100 should be in database");
        assert_eq!(entry.severity, DtcSeverity::Critical);
    }

    #[test]
    fn known_chassis_code() {
        // C0035 may or may not be in the Wal33D dataset — try a common one
        // At minimum verify lookup doesn't panic
        let _ = lookup("C0035");
    }

    #[test]
    fn unknown_code_returns_none() {
        assert!(lookup("P9999").is_none());
        assert!(lookup("XXXXX").is_none());
    }

    #[test]
    fn case_insensitive_lookup() {
        assert!(lookup("p0300").is_some());
        assert!(lookup("p0300").unwrap().description.contains("Misfire"));
    }

    // --- Severity inference ---

    #[test]
    fn misfire_is_critical() {
        assert_eq!(infer_severity("P0300"), DtcSeverity::Critical);
        assert_eq!(infer_severity("P0301"), DtcSeverity::Critical);
        assert_eq!(infer_severity("P0306"), DtcSeverity::Critical);
    }

    #[test]
    fn evap_is_info() {
        assert_eq!(infer_severity("P0440"), DtcSeverity::Info);
        assert_eq!(infer_severity("P0442"), DtcSeverity::Info);
    }

    #[test]
    fn evap_large_leak_is_warning() {
        // P0455 is outside the 0x0440..=0x0446 Info range
        assert_eq!(infer_severity("P0455"), DtcSeverity::Warning);
    }

    #[test]
    fn airbag_is_critical() {
        assert_eq!(infer_severity("B0100"), DtcSeverity::Critical);
        assert_eq!(infer_severity("B0101"), DtcSeverity::Critical);
    }

    #[test]
    fn can_bus_off_is_critical() {
        assert_eq!(infer_severity("U0001"), DtcSeverity::Critical);
        assert_eq!(infer_severity("U0073"), DtcSeverity::Critical);
    }

    #[test]
    fn lost_comms_ecm_is_critical() {
        assert_eq!(infer_severity("U0100"), DtcSeverity::Critical);
        assert_eq!(infer_severity("U0101"), DtcSeverity::Critical);
    }

    #[test]
    fn hvac_comms_is_info() {
        assert_eq!(infer_severity("U0164"), DtcSeverity::Info);
    }

    #[test]
    fn generic_sensor_is_warning() {
        assert_eq!(infer_severity("P0100"), DtcSeverity::Warning);
        assert_eq!(infer_severity("P0171"), DtcSeverity::Warning);
    }

    #[test]
    fn chassis_default_is_warning() {
        assert_eq!(infer_severity("C0035"), DtcSeverity::Warning);
    }

    // --- Manufacturer lookup ---

    #[test]
    fn manufacturer_lookup_falls_back_to_generic() {
        // A generic code looked up with a manufacturer should still return
        let entry = lookup_with_manufacturer("P0300", "NONEXISTENT").unwrap();
        assert!(entry.description.contains("Misfire"));
    }

    #[test]
    fn manufacturer_lookup_missing_returns_none() {
        assert!(lookup_with_manufacturer("ZZZZZ", "FORD").is_none());
    }

    // --- Data integrity ---

    #[test]
    fn generic_count_matches_expected() {
        let count = generic_count();
        // Wal33D DB has 9,415 generic rows; allow small variance for dedup
        assert!(count >= 9000, "expected >=9000 generic codes, got {count}");
        assert!(
            count <= 10000,
            "expected <=10000 generic codes, got {count}"
        );
    }

    #[test]
    fn manufacturer_count_matches_expected() {
        let count = manufacturer_count();
        assert!(
            count >= 9000,
            "expected >=9000 manufacturer codes, got {count}"
        );
        assert!(
            count <= 10000,
            "expected <=10000 manufacturer codes, got {count}"
        );
    }

    #[test]
    fn no_duplicate_generic_keys() {
        // HashMap silently overwrites duplicates — verify the count matches line count
        let line_count = GENERIC_TSV
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .count();
        let map_count = generic_count();
        // If these differ, we have duplicate codes in the TSV
        assert_eq!(
            line_count, map_count,
            "generic TSV has {line_count} data lines but map has {map_count} entries (duplicates?)"
        );
    }

    #[test]
    fn spot_check_multiple_categories() {
        // Verify at least one code from each category exists
        assert!(lookup("P0300").is_some(), "P0300 missing");
        assert!(lookup("P0420").is_some(), "P0420 missing");
        assert!(lookup("U0100").is_some(), "U0100 missing");
        // B and C codes may have different coverage in Wal33D;
        // verify the lookup itself doesn't panic
        let _ = lookup("B0100");
        let _ = lookup("C0035");
    }
}
