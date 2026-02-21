//! Raw CAN frame capture with optional ID filter and duration limit.

use async_trait::async_trait;
use std::time::{Duration, Instant};

use crate::error::{CanError, CanResult};
use crate::interface::CanInterface;
use crate::types::*;

/// Maximum capture duration (safety limit).
const MAX_DURATION_SECS: u64 = 30;

/// Maximum frames to capture per invocation.
const MAX_FRAMES: usize = 1000;

/// Captures raw CAN frames with optional ID filtering and duration limit.
pub struct CanMonitorTool;

#[async_trait]
impl CanTool for CanMonitorTool {
    fn name(&self) -> &str {
        "can_monitor"
    }

    fn description(&self) -> &str {
        "Capture raw CAN bus frames for a specified duration. Optionally filter by CAN ID. Max 30 seconds, 1000 frames."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "duration_secs": {
                    "type": "integer",
                    "description": "Capture duration in seconds (max 30)",
                    "default": 5
                },
                "filter_id": {
                    "type": "integer",
                    "description": "Optional CAN ID to filter (hex as decimal, e.g. 2024 for 0x7E8)"
                },
                "max_frames": {
                    "type": "integer",
                    "description": "Maximum number of frames to capture (max 1000)",
                    "default": 100
                }
            },
            "required": []
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        interface: &dyn CanInterface,
    ) -> CanResult<ToolResult> {
        let duration_secs = args
            .get("duration_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(5)
            .min(MAX_DURATION_SECS);

        let filter_id = args
            .get("filter_id")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let max_frames = args
            .get("max_frames")
            .and_then(|v| v.as_u64())
            .unwrap_or(100) as usize;
        let max_frames = max_frames.min(MAX_FRAMES);

        let deadline = Instant::now() + Duration::from_secs(duration_secs);
        let recv_timeout = Duration::from_millis(100);
        let mut captured: Vec<serde_json::Value> = Vec::new();

        while Instant::now() < deadline && captured.len() < max_frames {
            match interface.recv_frame(recv_timeout).await {
                Ok(frame) => {
                    if let Some(fid) = filter_id
                        && frame.id != fid
                    {
                        continue;
                    }

                    let hex_data: String = frame
                        .data
                        .iter()
                        .map(|b| format!("{b:02X}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    captured.push(serde_json::json!({
                        "id": format!("0x{:03X}", frame.id),
                        "data": hex_data,
                        "dlc": frame.data.len(),
                    }));
                }
                Err(CanError::Timeout { .. }) => {
                    continue;
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        let count = captured.len();
        let data = serde_json::json!({
            "frames": captured,
            "count": count,
            "duration_secs": duration_secs,
            "filter_id": filter_id.map(|id| format!("0x{id:03X}")),
        });

        let summary = match filter_id {
            Some(id) => format!("Captured {count} frames (filter: 0x{id:03X}) in {duration_secs}s"),
            None => format!("Captured {count} frames in {duration_secs}s"),
        };

        Ok(ToolResult::success(self.name(), data, summary))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockCanInterface;

    #[tokio::test]
    async fn monitor_captures_frames() {
        let mock = MockCanInterface::new();
        mock.queue_response(CanFrame::new(0x100, vec![0x01, 0x02, 0x03]));
        mock.queue_response(CanFrame::new(0x200, vec![0x04, 0x05]));
        mock.queue_response(CanFrame::new(0x100, vec![0x06]));

        let tool = CanMonitorTool;
        let result = tool
            .execute(
                serde_json::json!({"duration_secs": 1, "max_frames": 10}),
                &mock,
            )
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.data.unwrap()["count"], 3);
    }

    #[tokio::test]
    async fn monitor_with_filter() {
        let mock = MockCanInterface::new();
        mock.queue_response(CanFrame::new(0x100, vec![0x01]));
        mock.queue_response(CanFrame::new(0x200, vec![0x02]));
        mock.queue_response(CanFrame::new(0x100, vec![0x03]));

        let tool = CanMonitorTool;
        let result = tool
            .execute(
                serde_json::json!({"duration_secs": 1, "filter_id": 256, "max_frames": 10}),
                &mock,
            )
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.data.unwrap()["count"], 2);
    }

    #[tokio::test]
    async fn monitor_respects_max_frames() {
        let mock = MockCanInterface::new();
        for i in 0..50u8 {
            mock.queue_response(CanFrame::new(0x100, vec![i]));
        }

        let tool = CanMonitorTool;
        let result = tool
            .execute(
                serde_json::json!({"duration_secs": 5, "max_frames": 10}),
                &mock,
            )
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.data.unwrap()["count"], 10);
    }

    #[tokio::test]
    async fn monitor_empty_bus() {
        let mock = MockCanInterface::new();
        let tool = CanMonitorTool;
        let result = tool
            .execute(serde_json::json!({"duration_secs": 1}), &mock)
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.data.unwrap()["count"], 0);
    }
}
