//! Mock CAN interface for testing.
//!
//! Supports scripted response queues and frame recording. All tests use this
//! instead of real CAN hardware so the suite runs in CI on any platform.

use async_trait::async_trait;
use std::sync::Mutex;
use std::time::Duration;

use crate::ecu_profile;
use crate::error::{CanError, CanResult};
use crate::interface::CanInterface;
use crate::safety;
use crate::types::{CanFrame, OBD_REQUEST_ID};
use crate::uds_safety;

/// Mock CAN interface with scripted responses and frame recording.
pub struct MockCanInterface {
    /// Queued responses returned by `recv_frame` (FIFO order).
    responses: Mutex<Vec<CanFrame>>,
    /// All frames passed to `send_frame` (for test assertions).
    sent_frames: Mutex<Vec<CanFrame>>,
    /// Whether to enforce OBD-II safety checks (default: true).
    enforce_safety: bool,
}

impl MockCanInterface {
    /// Create a new mock with no queued responses.
    pub fn new() -> Self {
        Self {
            responses: Mutex::new(Vec::new()),
            sent_frames: Mutex::new(Vec::new()),
            enforce_safety: true,
        }
    }

    /// Create a mock pre-loaded with response frames.
    pub fn with_responses(responses: Vec<CanFrame>) -> Self {
        Self {
            responses: Mutex::new(responses),
            sent_frames: Mutex::new(Vec::new()),
            enforce_safety: true,
        }
    }

    /// Queue an additional response frame.
    pub fn queue_response(&self, frame: CanFrame) {
        self.responses.lock().unwrap().push(frame);
    }

    /// Get copies of all frames that were sent.
    pub fn sent_frames(&self) -> Vec<CanFrame> {
        self.sent_frames.lock().unwrap().clone()
    }

    /// Get the last sent frame, if any.
    pub fn last_sent(&self) -> Option<CanFrame> {
        self.sent_frames.lock().unwrap().last().cloned()
    }
}

impl Default for MockCanInterface {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CanInterface for MockCanInterface {
    async fn send_frame(&self, frame: &CanFrame) -> CanResult<()> {
        if self.enforce_safety && frame.data.len() >= 2 {
            let pci_len = frame.data[0];

            // OBD-II safety: check mode for standard OBD-II broadcast requests.
            // ISO-TP frames (FC = 0x30, etc.) use the same CAN ID but are not
            // OBD service requests and must not be blocked.
            if frame.id == OBD_REQUEST_ID && (1..=7).contains(&pci_len) {
                let mode = frame.data[1];
                if !safety::is_mode_allowed(mode) {
                    return Err(CanError::SafetyViolation { mode });
                }
            }

            // UDS safety: check service ID for known ECU request IDs.
            // Skip ISO-TP Flow Control frames (PCI byte starts with 0x3x).
            let is_flow_control = (pci_len >> 4) == 0x03;
            if !is_flow_control {
                let is_ecu_request = ecu_profile::all_profiles()
                    .iter()
                    .any(|p| p.request_id == frame.id);
                if is_ecu_request && (1..=7).contains(&pci_len) {
                    let service_id = frame.data[1];
                    if !uds_safety::is_uds_service_allowed(service_id) {
                        return Err(CanError::UdsSafetyViolation {
                            service_id,
                            service_name: uds_safety::uds_service_name(service_id).to_string(),
                        });
                    }
                }
            }
        }

        self.sent_frames.lock().unwrap().push(frame.clone());
        Ok(())
    }

    async fn recv_frame(&self, timeout: Duration) -> CanResult<CanFrame> {
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            return Err(CanError::Timeout {
                timeout_ms: timeout.as_millis() as u64,
            });
        }
        Ok(responses.remove(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn records_sent_frames() {
        let mock = MockCanInterface::new();
        let frame = CanFrame::new(OBD_REQUEST_ID, vec![0x02, 0x01, 0x0C, 0, 0, 0, 0, 0]);
        mock.send_frame(&frame).await.unwrap();

        let sent = mock.sent_frames();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].id, OBD_REQUEST_ID);
    }

    #[tokio::test]
    async fn returns_queued_responses() {
        let response = CanFrame::new(0x7E8, vec![0x04, 0x41, 0x0C, 0x1B, 0x58, 0, 0, 0]);
        let mock = MockCanInterface::with_responses(vec![response.clone()]);

        let received = mock.recv_frame(Duration::from_millis(100)).await.unwrap();
        assert_eq!(received, response);
    }

    #[tokio::test]
    async fn timeout_when_empty() {
        let mock = MockCanInterface::new();
        let result = mock.recv_frame(Duration::from_millis(100)).await;
        assert!(matches!(result, Err(CanError::Timeout { .. })));
    }

    #[tokio::test]
    async fn enforces_safety() {
        let mock = MockCanInterface::new();
        // Mode 0x04 (clear DTCs) — blocked
        let frame = CanFrame::new(OBD_REQUEST_ID, vec![0x01, 0x04, 0, 0, 0, 0, 0, 0]);
        let result = mock.send_frame(&frame).await;
        assert!(matches!(
            result,
            Err(CanError::SafetyViolation { mode: 0x04 })
        ));
    }

    #[tokio::test]
    async fn allows_safe_modes() {
        let mock = MockCanInterface::new();

        // Mode 0x01 (current data)
        let frame = CanFrame::new(OBD_REQUEST_ID, vec![0x02, 0x01, 0x0C, 0, 0, 0, 0, 0]);
        mock.send_frame(&frame).await.unwrap();

        // Mode 0x03 (stored DTCs)
        let frame = CanFrame::new(OBD_REQUEST_ID, vec![0x01, 0x03, 0, 0, 0, 0, 0, 0]);
        mock.send_frame(&frame).await.unwrap();

        // Mode 0x09 (vehicle info)
        let frame = CanFrame::new(OBD_REQUEST_ID, vec![0x02, 0x09, 0x02, 0, 0, 0, 0, 0]);
        mock.send_frame(&frame).await.unwrap();

        assert_eq!(mock.sent_frames().len(), 3);
    }

    #[tokio::test]
    async fn queue_after_construction() {
        let mock = MockCanInterface::new();
        let frame = CanFrame::new(0x7E8, vec![0x03, 0x41, 0x0D, 0x3C, 0, 0, 0, 0]);
        mock.queue_response(frame.clone());

        let received = mock.recv_frame(Duration::from_millis(100)).await.unwrap();
        assert_eq!(received, frame);
    }

    // ── UDS safety enforcement ──────────────────────────────────

    #[tokio::test]
    async fn uds_allows_read_did() {
        let mock = MockCanInterface::new();
        // UDS ReadDataByIdentifier (0x22) on BCR request ID 0x60D
        let frame = CanFrame::new(0x60D, vec![0x03, 0x22, 0xFD, 0x05, 0, 0, 0, 0]);
        mock.send_frame(&frame).await.unwrap();
        assert_eq!(mock.sent_frames().len(), 1);
    }

    #[tokio::test]
    async fn uds_allows_tester_present() {
        let mock = MockCanInterface::new();
        let frame = CanFrame::new(0x60D, vec![0x02, 0x3E, 0x00, 0, 0, 0, 0, 0]);
        mock.send_frame(&frame).await.unwrap();
    }

    #[tokio::test]
    async fn uds_blocks_write_did() {
        let mock = MockCanInterface::new();
        // UDS WriteDataByIdentifier (0x2E) — blocked
        let frame = CanFrame::new(0x60D, vec![0x04, 0x2E, 0xFD, 0x05, 0x00, 0, 0, 0]);
        let result = mock.send_frame(&frame).await;
        assert!(matches!(
            result,
            Err(CanError::UdsSafetyViolation {
                service_id: 0x2E,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn uds_blocks_security_access() {
        let mock = MockCanInterface::new();
        // UDS SecurityAccess (0x27) — blocked
        let frame = CanFrame::new(0x60D, vec![0x02, 0x27, 0x01, 0, 0, 0, 0, 0]);
        let result = mock.send_frame(&frame).await;
        assert!(matches!(
            result,
            Err(CanError::UdsSafetyViolation {
                service_id: 0x27,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn uds_allows_flow_control() {
        let mock = MockCanInterface::new();
        // ISO-TP Flow Control frame (0x30) on BCR request ID — must pass through
        let frame = CanFrame::new(0x60D, vec![0x30, 0x00, 0x00, 0, 0, 0, 0, 0]);
        mock.send_frame(&frame).await.unwrap();
    }

    #[tokio::test]
    async fn uds_safety_bcf_request_id() {
        let mock = MockCanInterface::new();
        // UDS ReadDTCInformation (0x19) on BCF request ID 0x609 — allowed
        let frame = CanFrame::new(0x609, vec![0x03, 0x19, 0x02, 0xFF, 0, 0, 0, 0]);
        mock.send_frame(&frame).await.unwrap();
    }

    #[tokio::test]
    async fn non_ecu_id_passes_without_uds_check() {
        let mock = MockCanInterface::new();
        // Arbitrary CAN ID that isn't OBD-II or any known ECU — should pass
        let frame = CanFrame::new(0x123, vec![0x02, 0x2E, 0xAA, 0, 0, 0, 0, 0]);
        mock.send_frame(&frame).await.unwrap();
    }
}
