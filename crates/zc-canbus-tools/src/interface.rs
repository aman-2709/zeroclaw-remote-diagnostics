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
#[cfg(target_os = "linux")]
use crate::{ecu_profile, uds_safety};
#[cfg(target_os = "linux")]
use socketcan::{EmbeddedFrame, Frame};

/// Trait for CAN bus interface implementations.
#[async_trait]
pub trait CanInterface: Send + Sync {
    /// Send a CAN frame. Enforces safety mode whitelist for OBD-II requests.
    async fn send_frame(&self, frame: &CanFrame) -> CanResult<()>;

    /// Receive a CAN frame, blocking up to `timeout`.
    async fn recv_frame(&self, timeout: Duration) -> CanResult<CanFrame>;

    /// Drain stale frames from the receive buffer.
    ///
    /// On real SocketCAN interfaces, CAN frames accumulate in the kernel
    /// buffer between queries. Call this before sending a UDS request to
    /// avoid spending the timeout reading through stale traffic.
    /// Default is a no-op (correct for mock interfaces).
    async fn drain_rx_buffer(&self) {}

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
///
/// Wraps `socketcan::tokio::CanSocket` for async CAN bus I/O.
/// Safety enforcement (OBD-II mode allowlist + UDS service allowlist) is
/// checked before any frame reaches the bus.
#[cfg(target_os = "linux")]
pub struct SocketCanInterface {
    socket: socketcan::tokio::CanSocket,
}

#[cfg(target_os = "linux")]
impl SocketCanInterface {
    /// Open a SocketCAN interface by name (e.g., "can0").
    pub fn new(interface_name: &str) -> CanResult<Self> {
        let socket = socketcan::tokio::CanSocket::open(interface_name).map_err(|e| {
            CanError::Interface(format!("failed to open {interface_name}: {e}"))
        })?;
        tracing::info!(interface = interface_name, "SocketCAN interface opened");
        Ok(Self { socket })
    }
}

#[cfg(target_os = "linux")]
#[async_trait]
impl CanInterface for SocketCanInterface {
    async fn send_frame(&self, frame: &CanFrame) -> CanResult<()> {
        // ── Safety checks ────────────────────────────────────────
        if frame.data.len() >= 2 {
            let pci_len = frame.data[0];

            // OBD-II safety: check mode for broadcast requests.
            if frame.id == OBD_REQUEST_ID && (1..=7).contains(&pci_len) {
                let mode = frame.data[1];
                if !safety::is_mode_allowed(mode) {
                    return Err(CanError::SafetyViolation { mode });
                }
            }

            // UDS safety: check service ID for known ECU request IDs.
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

        // ── Send via SocketCAN ───────────────────────────────────
        let sc_frame = socketcan::CanFrame::from_raw_id(frame.id, &frame.data).ok_or_else(
            || CanError::Interface(format!("invalid CAN frame: id=0x{:03X}", frame.id)),
        )?;

        self.socket
            .write_frame(sc_frame)
            .await
            .map_err(|e| CanError::Interface(format!("send failed: {e}")))?;

        tracing::trace!(id = format!("0x{:03X}", frame.id), "CAN TX");
        Ok(())
    }

    async fn recv_frame(&self, timeout: Duration) -> CanResult<CanFrame> {
        let result = tokio::time::timeout(timeout, self.socket.read_frame()).await;

        match result {
            Ok(Ok(sc_frame)) => {
                let id = sc_frame.raw_id();
                let data = sc_frame.data().to_vec();
                tracing::trace!(id = format!("0x{id:03X}"), len = data.len(), "CAN RX");
                Ok(CanFrame::new(id, data))
            }
            Ok(Err(e)) => Err(CanError::Interface(format!("recv failed: {e}"))),
            Err(_) => Err(CanError::Timeout {
                timeout_ms: timeout.as_millis() as u64,
            }),
        }
    }

    async fn drain_rx_buffer(&self) {
        let mut drained = 0u32;
        loop {
            match tokio::time::timeout(
                Duration::from_millis(1),
                self.socket.read_frame(),
            )
            .await
            {
                Ok(Ok(_)) => drained += 1,
                _ => break,
            }
        }
        if drained > 0 {
            tracing::debug!(frames = drained, "drained stale CAN frames");
        }
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
