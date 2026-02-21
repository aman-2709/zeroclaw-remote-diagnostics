//! Tool: Read stored DTCs (Mode 0x03).

use async_trait::async_trait;
use std::time::Duration;

use zc_protocol::dtc::{DtcCode, DtcSeverity};

use crate::dtc_db;
use crate::error::CanResult;
use crate::interface::CanInterface;
use crate::obd;
use crate::types::{CanTool, RESPONSE_SID_OFFSET, ToolResult};

/// Reads stored Diagnostic Trouble Codes from the vehicle ECU.
pub struct ReadDtcs;

#[async_trait]
impl CanTool for ReadDtcs {
    fn name(&self) -> &str {
        "read_dtcs"
    }

    fn description(&self) -> &str {
        "Read stored Diagnostic Trouble Codes (Mode 0x03) from the ECU"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "timeout_ms": { "type": "integer", "description": "Response timeout in milliseconds", "default": 2000 }
            }
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        interface: &dyn CanInterface,
    ) -> CanResult<ToolResult> {
        let timeout_ms = args
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(2000);
        let timeout = Duration::from_millis(timeout_ms);

        let request = obd::build_dtc_request();
        let response = obd::obd_query(interface, &request, timeout).await?;

        // Mode 03 response: [length, SID(0x43), num_dtcs, dtc1_hi, dtc1_lo, ...]
        let expected_sid = 0x03 + RESPONSE_SID_OFFSET; // 0x43
        if response.data.len() < 3 || response.data[1] != expected_sid {
            return Ok(ToolResult::failure(self.name(), "Invalid Mode 03 response"));
        }

        let num_dtcs_reported = response.data[2] as usize;

        // Parse DTC byte pairs starting at index 3
        let mut dtcs = Vec::new();
        let dtc_bytes = &response.data[3..];
        let mut i = 0;
        while i + 1 < dtc_bytes.len() {
            if let Some(code) = obd::decode_dtc_bytes(dtc_bytes[i], dtc_bytes[i + 1]) {
                let category = DtcCode::parse_category(&code);
                let (description, severity) = dtc_db::lookup(&code)
                    .map(|e| (Some(e.description.to_string()), e.severity))
                    .unwrap_or((None, DtcSeverity::Unknown));

                dtcs.push(DtcCode {
                    code,
                    category,
                    severity,
                    description,
                    mil_status: false,
                    freeze_frame: None,
                });
            }
            i += 2;
        }

        let summary = if dtcs.is_empty() {
            "No stored DTCs found".to_string()
        } else {
            let codes: Vec<&str> = dtcs.iter().map(|d| d.code.as_str()).collect();
            format!(
                "Found {} DTC(s) (reported {}): {}",
                dtcs.len(),
                num_dtcs_reported,
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
    async fn read_two_dtcs() {
        // Response: 2 DTCs — P0300 (0x03,0x00) and P0171 (0x01,0x71)
        let response = CanFrame::new(0x7E8, vec![0x06, 0x43, 0x02, 0x03, 0x00, 0x01, 0x71, 0x00]);
        let mock = MockCanInterface::with_responses(vec![response]);

        let result = ReadDtcs
            .execute(serde_json::json!({}), &mock)
            .await
            .unwrap();

        assert!(result.success);
        let summary = result.summary.unwrap();
        assert!(summary.contains("P0300"));
        assert!(summary.contains("P0171"));

        let data = result.data.unwrap();
        let dtcs: Vec<serde_json::Value> = serde_json::from_value(data).unwrap();
        assert_eq!(dtcs.len(), 2);
    }

    #[tokio::test]
    async fn read_zero_dtcs() {
        // Response: 0 DTCs, zero-padded
        let response = CanFrame::new(0x7E8, vec![0x02, 0x43, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        let mock = MockCanInterface::with_responses(vec![response]);

        let result = ReadDtcs
            .execute(serde_json::json!({}), &mock)
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.summary.unwrap().contains("No stored DTCs"));
    }
}
