//! Tool: Read DTCs via UDS 0x19 (ReadDTCInformation).

use async_trait::async_trait;
use std::time::Duration;

use zc_protocol::dtc::{DtcCode, DtcSeverity};

use crate::ecu_profile;
use crate::error::{CanError, CanResult};
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
        // UDS DTCs are 3 bytes (not 2 like OBD-II), but we also support
        // 2-byte DTCs decoded via obd::decode_dtc_bytes for compatibility.
        let mut dtcs = Vec::new();

        if response_data.len() >= 3 {
            // Skip positive response SID (0x59), sub-function echo, and status availability mask
            let dtc_data = &response_data[3..];

            // UDS DTCs: 3 bytes DTC + 1 byte status = 4 bytes per DTC
            let mut i = 0;
            while i + 3 < dtc_data.len() {
                let dtc_hi = dtc_data[i];
                let dtc_mid = dtc_data[i + 1];
                let dtc_lo = dtc_data[i + 2];
                let _status = dtc_data[i + 3];

                // Skip padding (all zeros)
                if dtc_hi == 0 && dtc_mid == 0 && dtc_lo == 0 {
                    i += 4;
                    continue;
                }

                // Decode using OBD-II 2-byte format (hi, mid) as primary code,
                // with lo byte appended as hex suffix
                let base_code = obd::decode_dtc_bytes(dtc_hi, dtc_mid);
                let code = match base_code {
                    Some(c) => format!("{c}{dtc_lo:02X}"),
                    None => format!("{dtc_hi:02X}{dtc_mid:02X}{dtc_lo:02X}"),
                };

                dtcs.push(DtcCode {
                    code: code.clone(),
                    category: DtcCode::parse_category(&code),
                    severity: DtcSeverity::Unknown,
                    description: None,
                    mil_status: false,
                    freeze_frame: None,
                });

                i += 4;
            }
        }

        let summary = if dtcs.is_empty() {
            format!("No DTCs found on {}", profile.name)
        } else {
            let codes: Vec<&str> = dtcs.iter().map(|d| d.code.as_str()).collect();
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
        // ISO-TP single-frame: PCI=0x07, payload: 0x59 0x02 0xFF + one DTC (03 00 00, status 0x09)
        // uds_query_isotp returns the full payload [0x59, 0x02, 0xFF, 0x03, 0x00, 0x00, 0x09]
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x07, 0x59, 0x02, 0xFF, 0x03, 0x00, 0x00, 0x09],
        ));

        let args = serde_json::json!({"ecu": "BCR"});
        let result = ReadUdsDtcs.execute(args, &mock).await.unwrap();

        assert!(result.success);
        let summary = result.summary.unwrap();
        assert!(summary.contains("1 DTC"));
        assert!(summary.contains("BCR"));
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
}
