//! Tool: Read Vehicle Identification Number (Mode 0x09, PID 0x02).
//!
//! VIN responses are 20 bytes (SID + PID + count + 17 chars) which exceeds
//! a single CAN frame, so this tool uses ISO-TP multi-frame reassembly.

use async_trait::async_trait;
use std::time::Duration;

use crate::error::{CanError, CanResult};
use crate::interface::CanInterface;
use crate::obd;
use crate::safety;
use crate::types::{CanTool, MODE_VEHICLE_INFO, ToolResult};

/// Reads the 17-character VIN via OBD-II Mode 0x09 PID 0x02.
pub struct ReadVin;

#[async_trait]
impl CanTool for ReadVin {
    fn name(&self) -> &str {
        "read_vin"
    }

    fn description(&self) -> &str {
        "Read the Vehicle Identification Number (VIN) via Mode 0x09 PID 0x02"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "timeout_ms": { "type": "integer", "description": "Response timeout in milliseconds", "default": 3000 }
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
            .unwrap_or(3000);
        let timeout = Duration::from_millis(timeout_ms);

        // Safety check
        if !safety::is_mode_allowed(MODE_VEHICLE_INFO) {
            return Err(CanError::SafetyViolation {
                mode: MODE_VEHICLE_INFO,
            });
        }

        // Send Mode 09, PID 02 request
        let request = obd::build_request(MODE_VEHICLE_INFO, 0x02);
        interface.send_frame(&request).await?;

        // Receive ISO-TP response (VIN is 20 bytes → multi-frame)
        let payload = obd::isotp_recv(interface, 0x7E8, timeout).await?;

        // Parse VIN from payload: [SID(0x49), PID(0x02), count(0x01), ...17 VIN chars]
        if payload.len() < 20 {
            return Ok(ToolResult::failure(
                self.name(),
                format!("VIN response too short: {} bytes (need 20)", payload.len()),
            ));
        }

        if payload[0] != 0x49 || payload[1] != 0x02 {
            return Ok(ToolResult::failure(
                self.name(),
                format!(
                    "Invalid VIN response SID/PID: 0x{:02X} 0x{:02X}",
                    payload[0], payload[1]
                ),
            ));
        }

        let vin_bytes = &payload[3..20];
        match std::str::from_utf8(vin_bytes) {
            Ok(vin) => {
                let data = serde_json::json!({ "vin": vin });
                let summary = format!("VIN: {vin}");
                Ok(ToolResult::success(self.name(), data, summary))
            }
            Err(e) => Ok(ToolResult::failure(
                self.name(),
                format!("VIN not valid UTF-8: {e}"),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockCanInterface;
    use crate::types::CanFrame;

    #[tokio::test]
    async fn read_vin_multi_frame() {
        // VIN = "WDBABC" + "DEFGHIJ" + "KLMNOPQ" = 17 chars (padded for test)
        // Full payload: 0x49, 0x02, 0x01, + 17 VIN = 20 bytes total
        let vin = b"1HGCM82633A004352"; // 17 chars
        let mut full_payload = vec![0x49, 0x02, 0x01];
        full_payload.extend_from_slice(vin);
        assert_eq!(full_payload.len(), 20);

        // First Frame: total = 20
        let mut ff_data = vec![0x10, 0x14]; // nibble 0x1, total_len = 20
        ff_data.extend_from_slice(&full_payload[..6]);
        let ff = CanFrame::new(0x7E8, ff_data);

        // CF1: seq 1, next 7 bytes
        let mut cf1_data = vec![0x21];
        cf1_data.extend_from_slice(&full_payload[6..13]);
        let cf1 = CanFrame::new(0x7E8, cf1_data);

        // CF2: seq 2, remaining 7 bytes
        let mut cf2_data = vec![0x22];
        cf2_data.extend_from_slice(&full_payload[13..20]);
        let cf2 = CanFrame::new(0x7E8, cf2_data);

        let mock = MockCanInterface::with_responses(vec![ff, cf1, cf2]);

        let result = ReadVin.execute(serde_json::json!({}), &mock).await.unwrap();

        assert!(result.success);
        let summary = result.summary.unwrap();
        assert!(summary.contains("1HGCM82633A004352"));

        let data = result.data.unwrap();
        assert_eq!(data["vin"], "1HGCM82633A004352");
    }
}
