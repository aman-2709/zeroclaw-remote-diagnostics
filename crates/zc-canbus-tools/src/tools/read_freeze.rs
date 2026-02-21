//! Tool: Read freeze frame data (Mode 0x02).
//!
//! Reads multiple PIDs from the freeze frame snapshot captured when a DTC was
//! set. Queries a standard set of PIDs and assembles a `FreezeFrame` struct.

use async_trait::async_trait;
use std::time::Duration;

use zc_protocol::dtc::FreezeFrame;

use crate::error::CanResult;
use crate::interface::CanInterface;
use crate::obd;
use crate::types::{CanTool, MODE_FREEZE_FRAME, ToolResult};

/// Standard PIDs to read from freeze frame data.
const FREEZE_FRAME_PIDS: &[u8] = &[
    0x04, // Engine load
    0x05, // Coolant temperature
    0x06, // Short-term fuel trim
    0x07, // Long-term fuel trim
    0x0C, // Engine RPM
    0x0D, // Vehicle speed
];

/// Reads freeze frame data for a set of standard PIDs.
pub struct ReadFreeze;

#[async_trait]
impl CanTool for ReadFreeze {
    fn name(&self) -> &str {
        "read_freeze"
    }

    fn description(&self) -> &str {
        "Read freeze frame data (Mode 0x02) captured when a DTC was set"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "timeout_ms": { "type": "integer", "description": "Per-PID response timeout in milliseconds", "default": 1000 }
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
            .unwrap_or(1000);
        let timeout = Duration::from_millis(timeout_ms);

        let mut ff = FreezeFrame {
            engine_rpm: None,
            vehicle_speed: None,
            coolant_temp: None,
            engine_load: None,
            fuel_system_status: None,
            short_term_fuel_trim: None,
            long_term_fuel_trim: None,
        };

        let mut sensor_count = 0usize;
        let mut errors = Vec::new();

        for &pid in FREEZE_FRAME_PIDS {
            let request = obd::build_request(MODE_FREEZE_FRAME, pid);
            match obd::obd_query(interface, &request, timeout).await {
                Ok(response) => {
                    if let Ok((_resp_pid, data)) =
                        obd::parse_pid_response(&response, MODE_FREEZE_FRAME)
                        && let Ok(pv) = obd::decode_pid(pid, data)
                    {
                        match pid {
                            0x04 => ff.engine_load = Some(pv.value),
                            0x05 => ff.coolant_temp = Some(pv.value),
                            0x06 => ff.short_term_fuel_trim = Some(pv.value),
                            0x07 => ff.long_term_fuel_trim = Some(pv.value),
                            0x0C => ff.engine_rpm = Some(pv.value),
                            0x0D => ff.vehicle_speed = Some(pv.value),
                            _ => {}
                        }
                        sensor_count += 1;
                    }
                }
                Err(e) => {
                    errors.push(format!("PID 0x{pid:02X}: {e}"));
                }
            }
        }

        let summary = if sensor_count == 0 {
            "No freeze frame data available".to_string()
        } else {
            let mut parts = Vec::new();
            if let Some(rpm) = ff.engine_rpm {
                parts.push(format!("RPM: {rpm:.0}"));
            }
            if let Some(speed) = ff.vehicle_speed {
                parts.push(format!("Speed: {speed:.0} km/h"));
            }
            if let Some(temp) = ff.coolant_temp {
                parts.push(format!("Coolant: {temp:.0}\u{00b0}C"));
            }
            format!(
                "Freeze frame: {sensor_count} sensor(s) read \u{2014} {}",
                parts.join(", ")
            )
        };

        let data = serde_json::json!({
            "freeze_frame": serde_json::to_value(&ff).unwrap_or_default(),
            "sensors_read": sensor_count,
            "errors": errors,
        });

        Ok(ToolResult::success(self.name(), data, summary))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockCanInterface;
    use crate::types::CanFrame;

    fn mode02_response(pid: u8, data: &[u8]) -> CanFrame {
        let mut frame_data = vec![0x02 + data.len() as u8, 0x42, pid];
        frame_data.extend_from_slice(data);
        frame_data.resize(8, 0x00);
        CanFrame::new(0x7E8, frame_data)
    }

    #[tokio::test]
    async fn read_freeze_frame() {
        // Queue responses for each PID in FREEZE_FRAME_PIDS order
        let responses = vec![
            mode02_response(0x04, &[0x80]),       // Engine load ~50%
            mode02_response(0x05, &[0x84]),       // Coolant 92 C
            mode02_response(0x06, &[0x80]),       // Short fuel trim 0%
            mode02_response(0x07, &[0x80]),       // Long fuel trim 0%
            mode02_response(0x0C, &[0x1B, 0x58]), // RPM 1750
            mode02_response(0x0D, &[0x3C]),       // Speed 60 km/h
        ];
        let mock = MockCanInterface::with_responses(responses);

        let result = ReadFreeze
            .execute(serde_json::json!({}), &mock)
            .await
            .unwrap();

        assert!(result.success);
        let summary = result.summary.unwrap();
        assert!(summary.contains("6 sensor(s)"));
        assert!(summary.contains("RPM: 1750"));
        assert!(summary.contains("Speed: 60"));
    }

    #[tokio::test]
    async fn read_freeze_no_data() {
        // Empty mock — all PIDs will timeout
        let mock = MockCanInterface::new();

        let result = ReadFreeze
            .execute(serde_json::json!({ "timeout_ms": 50 }), &mock)
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.summary.unwrap().contains("No freeze frame data"));
    }
}
