//! Tool: UDS DiagnosticSessionControl (0x10) + TesterPresent (0x3E).

use async_trait::async_trait;
use std::time::Duration;

use crate::ecu_profile;
use crate::error::{CanError, CanResult};
use crate::interface::CanInterface;
use crate::types::{CanTool, ToolResult};
use crate::uds;
use crate::uds_safety;

/// Controls the diagnostic session on a UDS ECU and sends TesterPresent.
pub struct UdsSessionControl;

/// Map a session name to its UDS byte value.
fn session_name_to_type(name: &str) -> Option<u8> {
    match name.to_lowercase().as_str() {
        "default" => Some(0x01),
        "extended" => Some(0x03),
        "programming" => Some(0x02),
        _ => None,
    }
}

/// Map a session type byte to its name.
fn session_type_to_name(session_type: u8) -> &'static str {
    match session_type {
        0x01 => "default",
        0x02 => "programming",
        0x03 => "extended",
        _ => "unknown",
    }
}

#[async_trait]
impl CanTool for UdsSessionControl {
    fn name(&self) -> &str {
        "uds_session_control"
    }

    fn description(&self) -> &str {
        "Control the diagnostic session on a UDS ECU (default/extended) or send TesterPresent"
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
                "session": {
                    "type": "string",
                    "description": "Session type: default, extended. Programming is blocked by safety policy.",
                    "enum": ["default", "extended"],
                    "default": "extended"
                },
                "tester_present": {
                    "type": "boolean",
                    "description": "If true, send TesterPresent (0x3E) instead of session control",
                    "default": false
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

        let tester_present = args
            .get("tester_present")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if tester_present {
            // Send TesterPresent (0x3E 0x00)
            let _response = uds::uds_query(interface, profile, 0x3E, &[0x00], timeout).await?;

            let summary = format!("TesterPresent acknowledged by {}", profile.name);
            let data = serde_json::json!({
                "ecu": profile.name,
                "service": "TesterPresent",
                "success": true,
            });
            return Ok(ToolResult::success(self.name(), data, summary));
        }

        // DiagnosticSessionControl
        let session_name = args
            .get("session")
            .and_then(|v| v.as_str())
            .unwrap_or("extended");

        let session_type = match session_name_to_type(session_name) {
            Some(t) => t,
            None => {
                return Ok(ToolResult::failure(
                    self.name(),
                    format!("Unknown session type: {session_name}. Use: default, extended"),
                ));
            }
        };

        // Safety check: programming session blocked
        if !uds_safety::is_session_type_allowed(session_type) {
            return Ok(ToolResult::failure(
                self.name(),
                format!(
                    "Session type '{}' (0x{:02X}) is blocked by safety policy",
                    session_name, session_type
                ),
            ));
        }

        let response = uds::uds_query(interface, profile, 0x10, &[session_type], timeout).await?;

        let active_session = response.first().copied().unwrap_or(session_type);
        let summary = format!(
            "{} session active on {} (0x{:02X})",
            session_type_to_name(active_session),
            profile.name,
            active_session
        );

        let data = serde_json::json!({
            "ecu": profile.name,
            "session": session_type_to_name(active_session),
            "session_type": format!("0x{:02X}", active_session),
        });

        Ok(ToolResult::success(self.name(), data, summary))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockCanInterface;
    use crate::types::CanFrame;

    #[tokio::test]
    async fn session_control_extended() {
        let mock = MockCanInterface::new();
        // Positive response: 0x50 0x03 (extended session confirmed)
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x02, 0x50, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00],
        ));

        let args = serde_json::json!({"ecu": "BCR", "session": "extended"});
        let result = UdsSessionControl.execute(args, &mock).await.unwrap();

        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data["session"], "extended");
        assert!(result.summary.unwrap().contains("extended"));
    }

    #[tokio::test]
    async fn session_control_default() {
        let mock = MockCanInterface::new();
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x02, 0x50, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00],
        ));

        let args = serde_json::json!({"ecu": "BCR", "session": "default"});
        let result = UdsSessionControl.execute(args, &mock).await.unwrap();

        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data["session"], "default");
    }

    #[tokio::test]
    async fn session_control_programming_blocked() {
        let mock = MockCanInterface::new();
        let args = serde_json::json!({"ecu": "BCR", "session": "programming"});
        let result = UdsSessionControl.execute(args, &mock).await.unwrap();

        assert!(!result.success);
        assert!(result.error.unwrap().contains("blocked by safety"));
        // No frame should have been sent
        assert!(mock.sent_frames().is_empty());
    }

    #[tokio::test]
    async fn tester_present() {
        let mock = MockCanInterface::new();
        // Positive response: 0x7E 0x00
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x02, 0x7E, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        ));

        let args = serde_json::json!({"ecu": "BCR", "tester_present": true});
        let result = UdsSessionControl.execute(args, &mock).await.unwrap();

        assert!(result.success);
        assert!(result.summary.unwrap().contains("TesterPresent"));
    }

    #[tokio::test]
    async fn missing_ecu_arg() {
        let mock = MockCanInterface::new();
        let args = serde_json::json!({"session": "extended"});
        let result = UdsSessionControl.execute(args, &mock).await.unwrap();

        assert!(!result.success);
        assert!(result.error.unwrap().contains("Missing"));
    }

    #[tokio::test]
    async fn unknown_ecu() {
        let mock = MockCanInterface::new();
        let args = serde_json::json!({"ecu": "XYZ"});
        let result = UdsSessionControl.execute(args, &mock).await;

        assert!(matches!(result, Err(CanError::UnknownEcu { .. })));
    }

    #[tokio::test]
    async fn unknown_session_type() {
        let mock = MockCanInterface::new();
        let args = serde_json::json!({"ecu": "BCR", "session": "turbo"});
        let result = UdsSessionControl.execute(args, &mock).await.unwrap();

        assert!(!result.success);
        assert!(result.error.unwrap().contains("Unknown session type"));
    }

    // ── Helper tests ─────────────────────────────────────────────

    #[test]
    fn session_name_mapping() {
        assert_eq!(session_name_to_type("default"), Some(0x01));
        assert_eq!(session_name_to_type("extended"), Some(0x03));
        assert_eq!(session_name_to_type("programming"), Some(0x02));
        assert_eq!(session_name_to_type("unknown"), None);
    }

    #[test]
    fn session_type_name_mapping() {
        assert_eq!(session_type_to_name(0x01), "default");
        assert_eq!(session_type_to_name(0x02), "programming");
        assert_eq!(session_type_to_name(0x03), "extended");
        assert_eq!(session_type_to_name(0xFF), "unknown");
    }
}
