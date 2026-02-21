//! Tool: Read OBD-II PID (Mode 0x01 — current data).

use async_trait::async_trait;
use std::time::Duration;

use crate::error::CanResult;
use crate::interface::CanInterface;
use crate::obd;
use crate::types::{CanTool, MODE_CURRENT_DATA, ToolResult};

/// Reads a live OBD-II PID and returns the decoded sensor value.
pub struct ReadPid;

#[async_trait]
impl CanTool for ReadPid {
    fn name(&self) -> &str {
        "read_pid"
    }

    fn description(&self) -> &str {
        "Read a live OBD-II PID (Mode 0x01) and return the decoded sensor value"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pid": { "type": "integer", "description": "OBD-II PID number (0x00-0xFF)" },
                "timeout_ms": { "type": "integer", "description": "Response timeout in milliseconds", "default": 1000 }
            },
            "required": ["pid"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        interface: &dyn CanInterface,
    ) -> CanResult<ToolResult> {
        let pid = match args.get("pid").and_then(|v| v.as_u64()) {
            Some(p) => p as u8,
            None => {
                return Ok(ToolResult::failure(
                    self.name(),
                    "Missing required argument: pid (u8)",
                ));
            }
        };

        let timeout_ms = args
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(1000);
        let timeout = Duration::from_millis(timeout_ms);

        let request = obd::build_request(MODE_CURRENT_DATA, pid);
        let response = obd::obd_query(interface, &request, timeout).await?;

        let (resp_pid, data) = obd::parse_pid_response(&response, MODE_CURRENT_DATA)?;

        if resp_pid != pid {
            return Ok(ToolResult::failure(
                self.name(),
                format!("PID mismatch: requested 0x{pid:02X}, got 0x{resp_pid:02X}"),
            ));
        }

        match obd::decode_pid(pid, data) {
            Ok(pv) => {
                let summary = format!("{}: {} {}", pv.name, pv.value, pv.unit);
                let data = serde_json::json!({
                    "pid": pid,
                    "name": pv.name,
                    "value": pv.value,
                    "unit": pv.unit,
                });
                Ok(ToolResult::success(self.name(), data, summary))
            }
            Err(e) => Ok(ToolResult::failure(self.name(), format!("{e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockCanInterface;
    use crate::types::CanFrame;

    #[tokio::test]
    async fn read_rpm() {
        // Response: RPM = 3500 ((0x36*256 + 0xB0) / 4 = 3500)
        let response = CanFrame::new(0x7E8, vec![0x04, 0x41, 0x0C, 0x36, 0xB0, 0, 0, 0]);
        let mock = MockCanInterface::with_responses(vec![response]);

        let args = serde_json::json!({ "pid": 0x0C });
        let result = ReadPid.execute(args, &mock).await.unwrap();

        assert!(result.success);
        assert!(result.summary.unwrap().contains("3500"));
    }

    #[tokio::test]
    async fn read_speed() {
        let response = CanFrame::new(0x7E8, vec![0x03, 0x41, 0x0D, 0x3C, 0, 0, 0, 0]);
        let mock = MockCanInterface::with_responses(vec![response]);

        let args = serde_json::json!({ "pid": 0x0D });
        let result = ReadPid.execute(args, &mock).await.unwrap();

        assert!(result.success);
        assert!(result.summary.unwrap().contains("60"));
    }

    #[tokio::test]
    async fn missing_pid_arg() {
        let mock = MockCanInterface::new();
        let args = serde_json::json!({});
        let result = ReadPid.execute(args, &mock).await.unwrap();

        assert!(!result.success);
        assert!(result.error.unwrap().contains("Missing"));
    }
}
