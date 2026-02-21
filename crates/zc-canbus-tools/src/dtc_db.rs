//! Static DTC database — match-based lookup for ~80 common codes.
//!
//! Returns description and severity. Upgradeable to `phf` later without API change.

use zc_protocol::dtc::DtcSeverity;

/// DTC entry from the static database.
#[derive(Debug, Clone)]
pub struct DtcEntry {
    pub description: &'static str,
    pub severity: DtcSeverity,
}

/// Look up a DTC code in the static database.
/// Input is case-insensitive (normalized to uppercase internally).
pub fn lookup(code: &str) -> Option<DtcEntry> {
    let code = code.to_uppercase();
    match code.as_str() {
        // ===== Powertrain — Fuel and Air Metering =====
        "P0100" => Some(DtcEntry {
            description: "Mass or Volume Air Flow Circuit Malfunction",
            severity: DtcSeverity::Warning,
        }),
        "P0101" => Some(DtcEntry {
            description: "Mass or Volume Air Flow Circuit Range/Performance",
            severity: DtcSeverity::Warning,
        }),
        "P0102" => Some(DtcEntry {
            description: "Mass or Volume Air Flow Circuit Low Input",
            severity: DtcSeverity::Warning,
        }),
        "P0103" => Some(DtcEntry {
            description: "Mass or Volume Air Flow Circuit High Input",
            severity: DtcSeverity::Warning,
        }),
        "P0110" => Some(DtcEntry {
            description: "Intake Air Temperature Circuit Malfunction",
            severity: DtcSeverity::Warning,
        }),
        "P0115" => Some(DtcEntry {
            description: "Engine Coolant Temperature Circuit Malfunction",
            severity: DtcSeverity::Warning,
        }),
        "P0116" => Some(DtcEntry {
            description: "Engine Coolant Temperature Circuit Range/Performance",
            severity: DtcSeverity::Warning,
        }),
        "P0117" => Some(DtcEntry {
            description: "Engine Coolant Temperature Circuit Low Input",
            severity: DtcSeverity::Warning,
        }),
        "P0118" => Some(DtcEntry {
            description: "Engine Coolant Temperature Circuit High Input",
            severity: DtcSeverity::Warning,
        }),
        "P0120" => Some(DtcEntry {
            description: "Throttle Position Sensor Circuit Malfunction",
            severity: DtcSeverity::Warning,
        }),
        "P0121" => Some(DtcEntry {
            description: "Throttle Position Sensor Circuit Range/Performance",
            severity: DtcSeverity::Warning,
        }),
        "P0122" => Some(DtcEntry {
            description: "Throttle Position Sensor Circuit Low Input",
            severity: DtcSeverity::Warning,
        }),
        "P0123" => Some(DtcEntry {
            description: "Throttle Position Sensor Circuit High Input",
            severity: DtcSeverity::Warning,
        }),
        "P0130" => Some(DtcEntry {
            description: "O2 Sensor Circuit Malfunction (Bank 1, Sensor 1)",
            severity: DtcSeverity::Warning,
        }),
        "P0131" => Some(DtcEntry {
            description: "O2 Sensor Circuit Low Voltage (Bank 1, Sensor 1)",
            severity: DtcSeverity::Warning,
        }),
        "P0133" => Some(DtcEntry {
            description: "O2 Sensor Circuit Slow Response (Bank 1, Sensor 1)",
            severity: DtcSeverity::Warning,
        }),
        "P0135" => Some(DtcEntry {
            description: "O2 Sensor Heater Circuit Malfunction (Bank 1, Sensor 1)",
            severity: DtcSeverity::Warning,
        }),

        // ===== Powertrain — Fuel and Air Metering (continued) =====
        "P0170" => Some(DtcEntry {
            description: "Fuel Trim Malfunction (Bank 1)",
            severity: DtcSeverity::Warning,
        }),
        "P0171" => Some(DtcEntry {
            description: "System Too Lean (Bank 1)",
            severity: DtcSeverity::Warning,
        }),
        "P0172" => Some(DtcEntry {
            description: "System Too Rich (Bank 1)",
            severity: DtcSeverity::Warning,
        }),
        "P0174" => Some(DtcEntry {
            description: "System Too Lean (Bank 2)",
            severity: DtcSeverity::Warning,
        }),
        "P0175" => Some(DtcEntry {
            description: "System Too Rich (Bank 2)",
            severity: DtcSeverity::Warning,
        }),

        // ===== Powertrain — Ignition System =====
        "P0300" => Some(DtcEntry {
            description: "Random/Multiple Cylinder Misfire Detected",
            severity: DtcSeverity::Critical,
        }),
        "P0301" => Some(DtcEntry {
            description: "Cylinder 1 Misfire Detected",
            severity: DtcSeverity::Critical,
        }),
        "P0302" => Some(DtcEntry {
            description: "Cylinder 2 Misfire Detected",
            severity: DtcSeverity::Critical,
        }),
        "P0303" => Some(DtcEntry {
            description: "Cylinder 3 Misfire Detected",
            severity: DtcSeverity::Critical,
        }),
        "P0304" => Some(DtcEntry {
            description: "Cylinder 4 Misfire Detected",
            severity: DtcSeverity::Critical,
        }),
        "P0305" => Some(DtcEntry {
            description: "Cylinder 5 Misfire Detected",
            severity: DtcSeverity::Critical,
        }),
        "P0306" => Some(DtcEntry {
            description: "Cylinder 6 Misfire Detected",
            severity: DtcSeverity::Critical,
        }),
        "P0335" => Some(DtcEntry {
            description: "Crankshaft Position Sensor A Circuit Malfunction",
            severity: DtcSeverity::Critical,
        }),
        "P0336" => Some(DtcEntry {
            description: "Crankshaft Position Sensor A Circuit Range/Performance",
            severity: DtcSeverity::Critical,
        }),
        "P0340" => Some(DtcEntry {
            description: "Camshaft Position Sensor Circuit Malfunction",
            severity: DtcSeverity::Critical,
        }),

        // ===== Powertrain — Emission Controls =====
        "P0400" => Some(DtcEntry {
            description: "Exhaust Gas Recirculation Flow Malfunction",
            severity: DtcSeverity::Warning,
        }),
        "P0401" => Some(DtcEntry {
            description: "Exhaust Gas Recirculation Flow Insufficient Detected",
            severity: DtcSeverity::Warning,
        }),
        "P0420" => Some(DtcEntry {
            description: "Catalyst System Efficiency Below Threshold (Bank 1)",
            severity: DtcSeverity::Warning,
        }),
        "P0421" => Some(DtcEntry {
            description: "Warm Up Catalyst Efficiency Below Threshold (Bank 1)",
            severity: DtcSeverity::Warning,
        }),
        "P0430" => Some(DtcEntry {
            description: "Catalyst System Efficiency Below Threshold (Bank 2)",
            severity: DtcSeverity::Warning,
        }),
        "P0440" => Some(DtcEntry {
            description: "Evaporative Emission Control System Malfunction",
            severity: DtcSeverity::Info,
        }),
        "P0441" => Some(DtcEntry {
            description: "Evaporative Emission Control System Incorrect Purge Flow",
            severity: DtcSeverity::Info,
        }),
        "P0442" => Some(DtcEntry {
            description: "Evaporative Emission Control System Leak Detected (small leak)",
            severity: DtcSeverity::Info,
        }),
        "P0443" => Some(DtcEntry {
            description: "Evaporative Emission Control System Purge Control Valve Circuit",
            severity: DtcSeverity::Info,
        }),
        "P0446" => Some(DtcEntry {
            description: "Evaporative Emission Control System Vent Control Circuit",
            severity: DtcSeverity::Info,
        }),
        "P0455" => Some(DtcEntry {
            description: "Evaporative Emission Control System Leak Detected (large leak)",
            severity: DtcSeverity::Warning,
        }),

        // ===== Powertrain — Vehicle Speed / Idle =====
        "P0500" => Some(DtcEntry {
            description: "Vehicle Speed Sensor Malfunction",
            severity: DtcSeverity::Warning,
        }),
        "P0505" => Some(DtcEntry {
            description: "Idle Control System Malfunction",
            severity: DtcSeverity::Warning,
        }),
        "P0506" => Some(DtcEntry {
            description: "Idle Control System RPM Lower Than Expected",
            severity: DtcSeverity::Info,
        }),
        "P0507" => Some(DtcEntry {
            description: "Idle Control System RPM Higher Than Expected",
            severity: DtcSeverity::Info,
        }),

        // ===== Powertrain — Transmission =====
        "P0700" => Some(DtcEntry {
            description: "Transmission Control System Malfunction",
            severity: DtcSeverity::Critical,
        }),
        "P0705" => Some(DtcEntry {
            description: "Transmission Range Sensor Circuit Malfunction",
            severity: DtcSeverity::Warning,
        }),
        "P0715" => Some(DtcEntry {
            description: "Input/Turbine Speed Sensor Circuit Malfunction",
            severity: DtcSeverity::Warning,
        }),
        "P0720" => Some(DtcEntry {
            description: "Output Speed Sensor Circuit Malfunction",
            severity: DtcSeverity::Warning,
        }),
        "P0730" => Some(DtcEntry {
            description: "Incorrect Gear Ratio",
            severity: DtcSeverity::Critical,
        }),
        "P0740" => Some(DtcEntry {
            description: "Torque Converter Clutch Circuit Malfunction",
            severity: DtcSeverity::Warning,
        }),
        "P0750" => Some(DtcEntry {
            description: "Shift Solenoid A Malfunction",
            severity: DtcSeverity::Warning,
        }),
        "P0755" => Some(DtcEntry {
            description: "Shift Solenoid B Malfunction",
            severity: DtcSeverity::Warning,
        }),

        // ===== Powertrain — Auxiliary Emission Controls =====
        "P0A80" => Some(DtcEntry {
            description: "Replace Hybrid Battery Pack",
            severity: DtcSeverity::Critical,
        }),

        // ===== Chassis =====
        "C0035" => Some(DtcEntry {
            description: "Left Front Wheel Speed Sensor Circuit",
            severity: DtcSeverity::Warning,
        }),
        "C0040" => Some(DtcEntry {
            description: "Right Front Wheel Speed Sensor Circuit",
            severity: DtcSeverity::Warning,
        }),
        "C0045" => Some(DtcEntry {
            description: "Left Rear Wheel Speed Sensor Circuit",
            severity: DtcSeverity::Warning,
        }),
        "C0050" => Some(DtcEntry {
            description: "Right Rear Wheel Speed Sensor Circuit",
            severity: DtcSeverity::Warning,
        }),
        "C0242" => Some(DtcEntry {
            description: "PCM Indicated TCS Malfunction",
            severity: DtcSeverity::Warning,
        }),
        "C0300" => Some(DtcEntry {
            description: "Rear Speed Sensor Malfunction",
            severity: DtcSeverity::Warning,
        }),

        // ===== Body =====
        "B0100" => Some(DtcEntry {
            description: "Electronic Frontal Sensor 1 Malfunction",
            severity: DtcSeverity::Critical,
        }),
        "B0101" => Some(DtcEntry {
            description: "Electronic Frontal Sensor 2 Malfunction",
            severity: DtcSeverity::Critical,
        }),
        "B1000" => Some(DtcEntry {
            description: "ECU Malfunction — Internal",
            severity: DtcSeverity::Critical,
        }),
        "B1200" => Some(DtcEntry {
            description: "Climate Control Push Button Circuit",
            severity: DtcSeverity::Info,
        }),
        "B1318" => Some(DtcEntry {
            description: "Battery Voltage Low",
            severity: DtcSeverity::Warning,
        }),
        "B1325" => Some(DtcEntry {
            description: "Battery Voltage Out Of Range",
            severity: DtcSeverity::Warning,
        }),
        "B1342" => Some(DtcEntry {
            description: "ECU Malfunction",
            severity: DtcSeverity::Critical,
        }),
        "B1601" => Some(DtcEntry {
            description: "PATS Received Incorrect Key Code",
            severity: DtcSeverity::Warning,
        }),
        "B2799" => Some(DtcEntry {
            description: "Engine Immobilizer System Malfunction",
            severity: DtcSeverity::Critical,
        }),

        // ===== Network/Communication =====
        "U0001" => Some(DtcEntry {
            description: "High Speed CAN Communication Bus",
            severity: DtcSeverity::Critical,
        }),
        "U0073" => Some(DtcEntry {
            description: "Control Module Communication Bus Off",
            severity: DtcSeverity::Critical,
        }),
        "U0100" => Some(DtcEntry {
            description: "Lost Communication With ECM/PCM",
            severity: DtcSeverity::Critical,
        }),
        "U0101" => Some(DtcEntry {
            description: "Lost Communication With TCM",
            severity: DtcSeverity::Critical,
        }),
        "U0121" => Some(DtcEntry {
            description: "Lost Communication With ABS",
            severity: DtcSeverity::Critical,
        }),
        "U0140" => Some(DtcEntry {
            description: "Lost Communication With Body Control Module",
            severity: DtcSeverity::Warning,
        }),
        "U0155" => Some(DtcEntry {
            description: "Lost Communication With Instrument Panel Cluster",
            severity: DtcSeverity::Warning,
        }),
        "U0164" => Some(DtcEntry {
            description: "Lost Communication With HVAC",
            severity: DtcSeverity::Info,
        }),
        "U0401" => Some(DtcEntry {
            description: "Invalid Data Received From ECM/PCM",
            severity: DtcSeverity::Warning,
        }),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_powertrain_code() {
        let entry = lookup("P0300").unwrap();
        assert!(entry.description.contains("Misfire"));
        assert_eq!(entry.severity, DtcSeverity::Critical);
    }

    #[test]
    fn known_chassis_code() {
        let entry = lookup("C0035").unwrap();
        assert!(entry.description.contains("Wheel Speed"));
        assert_eq!(entry.severity, DtcSeverity::Warning);
    }

    #[test]
    fn known_body_code() {
        let entry = lookup("B0100").unwrap();
        assert!(entry.description.contains("Frontal Sensor"));
        assert_eq!(entry.severity, DtcSeverity::Critical);
    }

    #[test]
    fn known_network_code() {
        let entry = lookup("U0100").unwrap();
        assert!(entry.description.contains("Lost Communication"));
        assert_eq!(entry.severity, DtcSeverity::Critical);
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
        assert!(lookup("c0035").is_some());
        assert!(lookup("u0100").is_some());
    }

    #[test]
    fn evap_codes_are_info() {
        let entry = lookup("P0440").unwrap();
        assert_eq!(entry.severity, DtcSeverity::Info);
    }

    #[test]
    fn transmission_critical() {
        let entry = lookup("P0700").unwrap();
        assert_eq!(entry.severity, DtcSeverity::Critical);
    }
}
