//! Tool: Read DTCs via UDS 0x19 (ReadDTCInformation).

use async_trait::async_trait;
use std::time::Duration;

use zc_protocol::dtc::{DtcCode, DtcSeverity};

use crate::dtc_db;
use crate::ecu_profile;
use crate::error::{CanError, CanResult};
use crate::ftb;
use crate::interface::CanInterface;
use crate::obd;
use crate::types::{CanTool, ToolResult};
use crate::uds;

/// Reads DTCs from a UDS-capable ECU (Hella BCR/BCF) via service 0x19.
pub struct ReadUdsDtcs;

#[async_trait]
impl CanTool for ReadUdsDtcs {
    fn name(&self) -> &str {
        "read_uds_dtcs"
    }

    fn description(&self) -> &str {
        "Read Diagnostic Trouble Codes from a UDS ECU (e.g., Hella BCR/BCF) via service 0x19"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "ecu": {
                    "type": "string",
                    "description": "ECU name: BCR or BCF",
                    "enum": ["BCR", "BCF"]
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Response timeout in milliseconds",
                    "default": 2000
                }
            },
            "required": ["ecu"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        interface: &dyn CanInterface,
    ) -> CanResult<ToolResult> {
        let ecu_name = match args.get("ecu").and_then(|v| v.as_str()) {
            Some(name) => name,
            None => {
                return Ok(ToolResult::failure(
                    self.name(),
                    "Missing required argument: ecu (string, e.g. \"BCR\")",
                ));
            }
        };

        let profile = match ecu_profile::find_profile(ecu_name) {
            Some(p) => p,
            None => {
                return Err(CanError::UnknownEcu {
                    name: ecu_name.to_string(),
                });
            }
        };

        let timeout_ms = args
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(2000);
        let timeout = Duration::from_millis(timeout_ms);

        // UDS 0x19, sub-function 0x02 (reportDTCByStatusMask), status mask 0xFF (all)
        // Use ISO-TP multi-frame because ECUs often return many DTCs.
        let response_data =
            uds::uds_query_isotp(interface, profile, 0x19, &[0x02, 0xFF], timeout).await?;

        // Response payload from ISO-TP (includes positive response SID):
        // [0x59, sub_fn, status_availability_mask, DTC_hi, DTC_mid, DTC_lo, status, ...]
        let mut dtcs = Vec::new();

        if response_data.len() >= 3 {
            // Skip positive response SID (0x59), sub-function echo, and status availability mask
            let dtc_data = &response_data[3..];

            // UDS DTCs: 3 bytes DTC + 1 byte status = 4 bytes per DTC
            let mut i = 0;
            while i + 3 < dtc_data.len() {
                let dtc_hi = dtc_data[i];
                let dtc_mid = dtc_data[i + 1];
                let dtc_lo = dtc_data[i + 2]; // Failure Type Byte
                let _status = dtc_data[i + 3];

                // Skip padding (all zeros)
                if dtc_hi == 0 && dtc_mid == 0 && dtc_lo == 0 {
                    i += 4;
                    continue;
                }

                // Preserve raw 3-byte DTC value
                let raw_dtc = format!("{dtc_hi:02X}{dtc_mid:02X}{dtc_lo:02X}");

                // Decode first 2 bytes → standard 5-char OBD-style code
                let code = obd::decode_dtc_bytes(dtc_hi, dtc_mid)
                    .unwrap_or_else(|| format!("{dtc_hi:02X}{dtc_mid:02X}"));

                // Decode 3rd byte → Failure Type Byte description
                let failure_type = ftb::format_ftb(dtc_lo);

                // Look up description and severity from database
                let (description, severity, severity_source): (Option<String>, DtcSeverity, &str) =
                    match dtc_db::lookup(&code) {
                        Some(entry) => (Some(entry.description), entry.severity, "database"),
                        None => (None, dtc_db::infer_severity(&code), "heuristic"),
                    };

                dtcs.push(DtcCode {
                    code: code.clone(),
                    category: DtcCode::parse_category(&code),
                    severity,
                    severity_source: Some(severity_source.to_string()),
                    description,
                    failure_type: Some(failure_type),
                    raw_dtc: Some(raw_dtc),
                    mil_status: false,
                    freeze_frame: None,
                });

                i += 4;
            }
        }

        let summary = if dtcs.is_empty() {
            format!("No DTCs found on {}", profile.name)
        } else {
            let codes: Vec<String> = dtcs
                .iter()
                .map(|d| {
                    if let Some(desc) = &d.description {
                        format!("{} ({})", d.code, desc)
                    } else {
                        d.code.clone()
                    }
                })
                .collect();
            format!(
                "Found {} DTC(s) on {}: {}",
                dtcs.len(),
                profile.name,
                codes.join(", ")
            )
        };

        let data = serde_json::to_value(&dtcs).unwrap_or_default();
        Ok(ToolResult::success(self.name(), data, summary))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockCanInterface;
    use crate::types::CanFrame;

    #[tokio::test]
    async fn read_bcr_dtcs_success() {
        let mock = MockCanInterface::new();
        // ISO-TP single-frame: PCI=0x07, payload: 0x59 0x02 0xFF + one DTC (03 00 42, status 0x09)
        // dtc_hi=0x03, dtc_mid=0x00 → P0300, dtc_lo=0x42 → FTB "General Checksum Failure"
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x07, 0x59, 0x02, 0xFF, 0x03, 0x00, 0x42, 0x09],
        ));

        let args = serde_json::json!({"ecu": "BCR"});
        let result = ReadUdsDtcs.execute(args, &mock).await.unwrap();

        assert!(result.success);
        let summary = result.summary.unwrap();
        assert!(summary.contains("1 DTC"));
        assert!(summary.contains("BCR"));

        // Verify the DTC structure
        let data = result.data.unwrap();
        let dtcs: Vec<serde_json::Value> = serde_json::from_value(data).unwrap();
        assert_eq!(dtcs.len(), 1);

        let dtc = &dtcs[0];
        // Code should be 5-char standard format (not 7-char with appended FTB)
        assert_eq!(dtc["code"], "P0300");
        // FTB decoded
        assert_eq!(dtc["failure_type"], "General Checksum Failure");
        // Raw bytes preserved
        assert_eq!(dtc["raw_dtc"], "030042");
        // Description from database
        assert!(dtc["description"].as_str().unwrap().contains("Misfire"));
        // Severity from database
        assert_eq!(dtc["severity"], "critical");
        assert_eq!(dtc["severity_source"], "database");
    }

    #[tokio::test]
    async fn read_bcr_dtc_with_ftb() {
        let mock = MockCanInterface::new();
        // DTC: dtc_hi=0x03, dtc_mid=0x00, dtc_lo=0x07 → P0300, FTB=Circuit Short to Ground
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x07, 0x59, 0x02, 0xFF, 0x03, 0x00, 0x07, 0x09],
        ));

        let args = serde_json::json!({"ecu": "BCR"});
        let result = ReadUdsDtcs.execute(args, &mock).await.unwrap();
        assert!(result.success);

        let data = result.data.unwrap();
        let dtcs: Vec<serde_json::Value> = serde_json::from_value(data).unwrap();
        assert_eq!(dtcs[0]["failure_type"], "Circuit Short to Ground");
        assert_eq!(dtcs[0]["raw_dtc"], "030007");
    }

    #[tokio::test]
    async fn read_bcr_no_dtcs() {
        let mock = MockCanInterface::new();
        // ISO-TP single-frame: PCI=0x03, payload: 0x59 0x02 0xFF (no DTCs)
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x03, 0x59, 0x02, 0xFF, 0x00, 0x00, 0x00, 0x00],
        ));

        let args = serde_json::json!({"ecu": "BCR"});
        let result = ReadUdsDtcs.execute(args, &mock).await.unwrap();

        assert!(result.success);
        assert!(result.summary.unwrap().contains("No DTCs"));
    }

    #[tokio::test]
    async fn read_missing_ecu_arg() {
        let mock = MockCanInterface::new();
        let args = serde_json::json!({});
        let result = ReadUdsDtcs.execute(args, &mock).await.unwrap();

        assert!(!result.success);
        assert!(result.error.unwrap().contains("Missing"));
    }

    #[tokio::test]
    async fn read_unknown_ecu() {
        let mock = MockCanInterface::new();
        let args = serde_json::json!({"ecu": "XYZ"});
        let result = ReadUdsDtcs.execute(args, &mock).await;

        assert!(matches!(result, Err(CanError::UnknownEcu { .. })));
    }

    #[tokio::test]
    async fn uds_dtc_code_is_five_chars() {
        let mock = MockCanInterface::new();
        // P0171 (0x01, 0x71) with FTB 0x11
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x07, 0x59, 0x02, 0xFF, 0x01, 0x71, 0x11, 0x09],
        ));

        let args = serde_json::json!({"ecu": "BCR"});
        let result = ReadUdsDtcs.execute(args, &mock).await.unwrap();

        let data = result.data.unwrap();
        let dtcs: Vec<serde_json::Value> = serde_json::from_value(data).unwrap();
        let code = dtcs[0]["code"].as_str().unwrap();
        // Must be exactly 5 chars (not 7 with appended FTB hex)
        assert_eq!(code.len(), 5, "code should be 5 chars, got: {code}");
        assert_eq!(code, "P0171");
    }

    #[tokio::test]
    async fn uds_dtc_unknown_code_gets_heuristic_severity() {
        let mock = MockCanInterface::new();
        // Use an obscure code unlikely to be in database: P3FFF (0x3F, 0xFF)
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x07, 0x59, 0x02, 0xFF, 0x3F, 0xFF, 0x00, 0x09],
        ));

        let args = serde_json::json!({"ecu": "BCR"});
        let result = ReadUdsDtcs.execute(args, &mock).await.unwrap();

        let data = result.data.unwrap();
        let dtcs: Vec<serde_json::Value> = serde_json::from_value(data).unwrap();
        // Should have severity_source = "heuristic" and no description
        let dtc = &dtcs[0];
        assert_eq!(dtc["severity_source"], "heuristic");
        assert!(dtc.get("description").is_none() || dtc["description"].is_null());
    }
}
