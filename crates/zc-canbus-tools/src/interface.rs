//! CAN bus interface abstraction.
//!
//! `CanInterface` trait with `send_frame`/`recv_frame`. Two impls:
//! - `SocketCanInterface` — Linux-only, wraps `socketcan::CanSocket`
//! - `MockCanInterface` — all platforms, scripted responses (in `mock.rs`)
//!
//! Safety enforcement happens at the interface level: `send_frame` rejects
//! disallowed OBD-II modes before any bytes hit the bus.

use async_trait::async_trait;
use std::time::Duration;

use crate::error::{CanError, CanResult};
use crate::safety;
use crate::types::{CanFrame, OBD_REQUEST_ID, OBD_RESPONSE_ID_MAX, OBD_RESPONSE_ID_MIN};

/// Trait for CAN bus interface implementations.
#[async_trait]
pub trait CanInterface: Send + Sync {
    /// Send a CAN frame. Enforces safety mode whitelist for OBD-II requests.
    async fn send_frame(&self, frame: &CanFrame) -> CanResult<()>;

    /// Receive a CAN frame, blocking up to `timeout`.
    async fn recv_frame(&self, timeout: Duration) -> CanResult<CanFrame>;

    /// Send an OBD-II request and collect response frame(s).
    ///
    /// Safety is enforced at the interface level. Builds the standard OBD-II
    /// request frame and waits for one matching response.
    async fn obd_request(
        &self,
        mode: u8,
        pid: Option<u8>,
        timeout: Duration,
    ) -> CanResult<Vec<CanFrame>> {
        if !safety::is_mode_allowed(mode) {
            return Err(CanError::SafetyViolation { mode });
        }

        let data = match pid {
            Some(p) => vec![0x02, mode, p, 0x00, 0x00, 0x00, 0x00, 0x00],
            None => vec![0x01, mode, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        };
        self.send_frame(&CanFrame::new(OBD_REQUEST_ID, data))
            .await?;

        let mut responses = Vec::new();
        let start = std::time::Instant::now();

        while start.elapsed() < timeout {
            let remaining = timeout.saturating_sub(start.elapsed());
            let wait = remaining.min(Duration::from_millis(100));

            match self.recv_frame(wait).await {
                Ok(f) if is_obd_response(f.id) => {
                    responses.push(f);
                    break; // single-frame: one response per request
                }
                Ok(_) => continue,
                Err(CanError::Timeout { .. }) => {
                    if !responses.is_empty() {
                        break;
                    }
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        if responses.is_empty() {
            return Err(CanError::Timeout {
                timeout_ms: timeout.as_millis() as u64,
            });
        }

        Ok(responses)
    }
}

/// Check if a CAN ID is an OBD-II response (0x7E8–0x7EF).
pub fn is_obd_response(id: u32) -> bool {
    (OBD_RESPONSE_ID_MIN..=OBD_RESPONSE_ID_MAX).contains(&id)
}

// ── SocketCAN (Linux-only) ──────────────────────────────────────

/// SocketCAN interface for Linux hosts.
#[cfg(target_os = "linux")]
pub struct SocketCanInterface {
    _interface_name: String,
}

#[cfg(target_os = "linux")]
impl SocketCanInterface {
    pub fn new(interface_name: &str) -> CanResult<Self> {
        Ok(Self {
            _interface_name: interface_name.to_string(),
        })
    }
}

#[cfg(target_os = "linux")]
#[async_trait]
impl CanInterface for SocketCanInterface {
    async fn send_frame(&self, frame: &CanFrame) -> CanResult<()> {
        // Safety check for OBD-II requests (data[0] = 0x01–0x07 = length byte).
        // ISO-TP transport frames (FC = 0x30, etc.) use the same CAN ID but
        // are not OBD service requests and must pass through.
        if frame.id == OBD_REQUEST_ID && frame.data.len() >= 2 && (1..=7).contains(&frame.data[0]) {
            let mode = frame.data[1];
            if !safety::is_mode_allowed(mode) {
                return Err(CanError::SafetyViolation { mode });
            }
        }
        // TODO: wire to socketcan::CanSocket when running on real hardware
        Err(CanError::Interface(
            "SocketCAN send not yet implemented".into(),
        ))
    }

    async fn recv_frame(&self, _timeout: Duration) -> CanResult<CanFrame> {
        // TODO: wire to socketcan::CanSocket when running on real hardware
        Err(CanError::Interface(
            "SocketCAN recv not yet implemented".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obd_response_id_range() {
        assert!(is_obd_response(0x7E8));
        assert!(is_obd_response(0x7EF));
        assert!(!is_obd_response(0x7E7));
        assert!(!is_obd_response(0x7F0));
        assert!(!is_obd_response(0x7DF)); // request ID, not response
    }
}
