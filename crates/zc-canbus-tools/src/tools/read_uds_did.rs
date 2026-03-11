//! Tool: Read Data Identifier via UDS 0x22 (ReadDataByIdentifier).

use async_trait::async_trait;
use std::time::Duration;

use crate::ecu_profile::{self, DidEntry, decode_did_value};
use crate::error::{CanError, CanResult};
use crate::interface::CanInterface;
use crate::types::{CanTool, ToolResult};
use crate::uds;

/// Reads one or all known DIDs from a UDS-capable ECU.
pub struct ReadUdsDid;

#[async_trait]
impl CanTool for ReadUdsDid {
    fn name(&self) -> &str {
        "read_uds_did"
    }

    fn description(&self) -> &str {
        "Read a Data Identifier (DID) from a UDS ECU via service 0x22. Omit did to read all known DIDs."
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
                "did": {
                    "type": "integer",
                    "description": "DID number (e.g., 0xFD05). Omit to read all known DIDs for this ECU."
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Response timeout in milliseconds per DID",
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

        let specific_did = args.get("did").and_then(|v| v.as_u64()).map(|d| d as u16);

        let dids_to_read: Vec<&DidEntry> = match specific_did {
            Some(did) => match ecu_profile::find_did(profile, did) {
                Some(entry) => vec![entry],
                None => {
                    return Ok(ToolResult::failure(
                        self.name(),
                        format!("Unknown DID 0x{did:04X} for ECU {}", profile.name),
                    ));
                }
            },
            None => profile.known_dids.iter().collect(),
        };

        let mut results = Vec::new();
        let mut errors = Vec::new();

        for entry in &dids_to_read {
            let did_bytes = [(entry.did >> 8) as u8, (entry.did & 0xFF) as u8];

            match uds::uds_query(interface, profile, 0x22, &did_bytes, timeout).await {
                Ok(response_data) => {
                    // Response data: [DID_hi, DID_lo, value_bytes...]
                    let value_data = if response_data.len() > 2 {
                        &response_data[2..]
                    } else {
                        &[]
                    };

                    let decoded = decode_did_value(&entry.decode, value_data);

                    results.push(serde_json::json!({
                        "did": format!("0x{:04X}", entry.did),
                        "name": entry.name,
                        "diag_code": entry.diag_code,
                        "decoded": decoded,
                    }));
                }
                Err(e) => {
                    errors.push(format!("DID 0x{:04X} ({}): {e}", entry.did, entry.name));
                }
            }
        }

        if results.is_empty() && !errors.is_empty() {
            return Ok(ToolResult::failure(
                self.name(),
                format!("All DID reads failed: {}", errors.join("; ")),
            ));
        }

        let summary = if errors.is_empty() {
            format!("Read {} DID(s) from {}", results.len(), profile.name)
        } else {
            format!(
                "Read {} DID(s) from {} ({} failed)",
                results.len(),
                profile.name,
                errors.len()
            )
        };

        let data = serde_json::json!({
            "ecu": profile.name,
            "dids": results,
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

    #[tokio::test]
    async fn read_single_did_voltage() {
        let mock = MockCanInterface::new();
        // Positive response for 0x22: SID=0x62, DID=FD05, value=04 CA (1226 * 0.01 = 12.26V)
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x05, 0x62, 0xFD, 0x05, 0x04, 0xCA, 0x00, 0x00],
        ));

        let args = serde_json::json!({"ecu": "BCR", "did": 0xFD05});
        let result = ReadUdsDid.execute(args, &mock).await.unwrap();

        assert!(result.success);
        let data = result.data.unwrap();
        let dids = data["dids"].as_array().unwrap();
        assert_eq!(dids.len(), 1);
        assert_eq!(dids[0]["name"], "Power Supply Voltage");

        let value = dids[0]["decoded"]["value"].as_f64().unwrap();
        assert!((value - 12.26).abs() < 0.01);
    }

    #[tokio::test]
    async fn read_single_did_bool() {
        let mock = MockCanInterface::new();
        // Positive response: DID=FD10, value=0x01 (brake light LEFT open load = true)
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x04, 0x62, 0xFD, 0x10, 0x01, 0x00, 0x00, 0x00],
        ));

        let args = serde_json::json!({"ecu": "BCR", "did": 0xFD10});
        let result = ReadUdsDid.execute(args, &mock).await.unwrap();

        assert!(result.success);
        let data = result.data.unwrap();
        let dids = data["dids"].as_array().unwrap();
        assert_eq!(dids[0]["decoded"]["value"], true);
    }

    #[tokio::test]
    async fn read_all_dids_bcr() {
        let mock = MockCanInterface::new();

        // Queue 4 responses for BCR's 4 known DIDs
        // DID FD05: voltage 12.26V
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x05, 0x62, 0xFD, 0x05, 0x04, 0xCA, 0x00, 0x00],
        ));
        // DID FD10: brake light LEFT = false
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x04, 0x62, 0xFD, 0x10, 0x00, 0x00, 0x00, 0x00],
        ));
        // DID FD11: brake light RIGHT = false
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x04, 0x62, 0xFD, 0x11, 0x00, 0x00, 0x00, 0x00],
        ));
        // DID FD50: reprogramming attempts = 3
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x05, 0x62, 0xFD, 0x50, 0x00, 0x03, 0x00, 0x00],
        ));

        let args = serde_json::json!({"ecu": "BCR"});
        let result = ReadUdsDid.execute(args, &mock).await.unwrap();

        assert!(result.success);
        let data = result.data.unwrap();
        let dids = data["dids"].as_array().unwrap();
        assert_eq!(dids.len(), 4);
        assert!(result.summary.unwrap().contains("4 DID(s)"));
    }

    #[tokio::test]
    async fn read_unknown_did() {
        let mock = MockCanInterface::new();
        let args = serde_json::json!({"ecu": "BCR", "did": 0x0001});
        let result = ReadUdsDid.execute(args, &mock).await.unwrap();

        assert!(!result.success);
        assert!(result.error.unwrap().contains("Unknown DID"));
    }

    #[tokio::test]
    async fn read_missing_ecu() {
        let mock = MockCanInterface::new();
        let args = serde_json::json!({});
        let result = ReadUdsDid.execute(args, &mock).await.unwrap();

        assert!(!result.success);
        assert!(result.error.unwrap().contains("Missing"));
    }

    #[tokio::test]
    async fn read_unknown_ecu() {
        let mock = MockCanInterface::new();
        let args = serde_json::json!({"ecu": "XYZ"});
        let result = ReadUdsDid.execute(args, &mock).await;

        assert!(matches!(result, Err(CanError::UnknownEcu { .. })));
    }
}
