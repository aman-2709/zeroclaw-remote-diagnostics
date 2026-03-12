use serde::{Deserialize, Serialize};

/// OBD-II / UDS Diagnostic Trouble Code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DtcCode {
    /// Standard DTC string (e.g., "P0300", "C0035").
    pub code: String,
    /// DTC category derived from first character.
    pub category: DtcCategory,
    /// Severity classification.
    pub severity: DtcSeverity,
    /// How severity was determined: "database" (exact match) or "heuristic" (pattern-based).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity_source: Option<String>,
    /// Human-readable description (from DTC database).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// UDS Failure Type Byte description (only for UDS DTCs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_type: Option<String>,
    /// Raw UDS DTC bytes as hex (e.g., "030042") — preserves original encoding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_dtc: Option<String>,
    /// Whether MIL (check engine light) is illuminated.
    pub mil_status: bool,
    /// Freeze frame data captured when DTC was set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freeze_frame: Option<FreezeFrame>,
}

/// DTC category based on first character of code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DtcCategory {
    /// P — Powertrain (engine, transmission).
    Powertrain,
    /// C — Chassis (ABS, steering).
    Chassis,
    /// B — Body (airbags, AC, lighting).
    Body,
    /// U — Network/Communication (CAN bus errors).
    Network,
}

/// Severity classification of a DTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DtcSeverity {
    /// Informational — no immediate action needed.
    Info,
    /// Warning — schedule maintenance.
    Warning,
    /// Critical — immediate attention required.
    Critical,
    /// Unknown — severity not in database.
    Unknown,
}

/// Freeze frame data captured at the moment a DTC was set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreezeFrame {
    /// Engine RPM at time of fault.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_rpm: Option<f64>,
    /// Vehicle speed in km/h.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vehicle_speed: Option<f64>,
    /// Engine coolant temperature in celsius.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coolant_temp: Option<f64>,
    /// Engine load percentage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_load: Option<f64>,
    /// Fuel system status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuel_system_status: Option<String>,
    /// Short-term fuel trim (%).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_term_fuel_trim: Option<f64>,
    /// Long-term fuel trim (%).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long_term_fuel_trim: Option<f64>,
}

impl DtcCode {
    /// Parse DTC category from the code string.
    pub fn parse_category(code: &str) -> DtcCategory {
        match code.chars().next() {
            Some('P' | 'p') => DtcCategory::Powertrain,
            Some('C' | 'c') => DtcCategory::Chassis,
            Some('B' | 'b') => DtcCategory::Body,
            Some('U' | 'u') => DtcCategory::Network,
            _ => DtcCategory::Powertrain, // Default per SAE J2012
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dtc_category_parsing() {
        assert_eq!(DtcCode::parse_category("P0300"), DtcCategory::Powertrain);
        assert_eq!(DtcCode::parse_category("C0035"), DtcCategory::Chassis);
        assert_eq!(DtcCode::parse_category("B0100"), DtcCategory::Body);
        assert_eq!(DtcCode::parse_category("U0100"), DtcCategory::Network);
    }

    #[test]
    fn dtc_roundtrip() {
        let dtc = DtcCode {
            code: "P0300".into(),
            category: DtcCategory::Powertrain,
            severity: DtcSeverity::Critical,
            severity_source: Some("database".into()),
            description: Some("Random/Multiple Cylinder Misfire Detected".into()),
            failure_type: None,
            raw_dtc: None,
            mil_status: true,
            freeze_frame: Some(FreezeFrame {
                engine_rpm: Some(2500.0),
                vehicle_speed: Some(60.0),
                coolant_temp: Some(92.0),
                engine_load: Some(45.0),
                fuel_system_status: Some("closed loop".into()),
                short_term_fuel_trim: Some(1.5),
                long_term_fuel_trim: Some(-2.3),
            }),
        };
        let json = serde_json::to_string_pretty(&dtc).unwrap();
        let deserialized: DtcCode = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.code, "P0300");
        assert!(deserialized.mil_status);
        assert!(deserialized.freeze_frame.is_some());
    }

    #[test]
    fn dtc_severity_serialization() {
        assert_eq!(
            serde_json::to_string(&DtcSeverity::Critical).unwrap(),
            r#""critical""#
        );
        assert_eq!(
            serde_json::to_string(&DtcSeverity::Info).unwrap(),
            r#""info""#
        );
    }

    #[test]
    fn dtc_with_failure_type_roundtrip() {
        let dtc = DtcCode {
            code: "P0300".into(),
            category: DtcCategory::Powertrain,
            severity: DtcSeverity::Critical,
            severity_source: Some("database".into()),
            description: Some("Random/Multiple Cylinder Misfire Detected".into()),
            failure_type: Some("Circuit Short to Ground".into()),
            raw_dtc: Some("030007".into()),
            mil_status: false,
            freeze_frame: None,
        };
        let json = serde_json::to_string(&dtc).unwrap();
        assert!(json.contains("failure_type"));
        assert!(json.contains("Circuit Short to Ground"));
        assert!(json.contains("raw_dtc"));
        assert!(json.contains("030007"));
        assert!(json.contains("severity_source"));

        let deserialized: DtcCode = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized.failure_type.as_deref(),
            Some("Circuit Short to Ground")
        );
        assert_eq!(deserialized.raw_dtc.as_deref(), Some("030007"));
        assert_eq!(deserialized.severity_source.as_deref(), Some("database"));
    }

    #[test]
    fn dtc_optional_fields_omitted_when_none() {
        let dtc = DtcCode {
            code: "P0171".into(),
            category: DtcCategory::Powertrain,
            severity: DtcSeverity::Warning,
            severity_source: None,
            description: None,
            failure_type: None,
            raw_dtc: None,
            mil_status: false,
            freeze_frame: None,
        };
        let json = serde_json::to_string(&dtc).unwrap();
        // Optional fields should be omitted
        assert!(!json.contains("failure_type"));
        assert!(!json.contains("raw_dtc"));
        assert!(!json.contains("severity_source"));
        assert!(!json.contains("description"));
        assert!(!json.contains("freeze_frame"));
    }

    #[test]
    fn dtc_deserializes_without_new_fields() {
        // Backward compatibility: old JSON without the new fields should still deserialize
        let json = r#"{
            "code": "P0300",
            "category": "powertrain",
            "severity": "critical",
            "mil_status": true
        }"#;
        let dtc: DtcCode = serde_json::from_str(json).unwrap();
        assert_eq!(dtc.code, "P0300");
        assert!(dtc.failure_type.is_none());
        assert!(dtc.raw_dtc.is_none());
        assert!(dtc.severity_source.is_none());
    }
}
