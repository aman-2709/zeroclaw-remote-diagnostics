//! UDS (ISO 14229) protocol helpers — parallel to `obd.rs`.
//!
//! Builds UDS request frames using ECU profile CAN IDs, sends/receives
//! via the `CanInterface` trait, handles negative responses and NRC codes.

use std::time::Duration;

use crate::ecu_profile::EcuProfile;
use crate::error::{CanError, CanResult};
use crate::interface::CanInterface;
use crate::types::*;
use crate::uds_safety;

/// Default receive timeout for UDS responses.
pub const DEFAULT_UDS_TIMEOUT: Duration = Duration::from_millis(2000);

/// UDS positive response SID offset (same as OBD-II: request + 0x40).
const POSITIVE_RESPONSE_OFFSET: u8 = 0x40;

/// UDS negative response service ID.
const NEGATIVE_RESPONSE_SID: u8 = 0x7F;

// ── Request builders ─────────────────────────────────────────────

/// Build a UDS single-frame request for a given ECU profile.
///
/// Frame layout: `[PCI, service_id, data...]` padded to 8 bytes.
/// PCI = ISO-TP Single Frame with length.
pub fn build_uds_request(profile: &EcuProfile, service_id: u8, data: &[u8]) -> CanFrame {
    let pci_len = 1 + data.len(); // service_id + data
    let mut frame_data = vec![0u8; 8];
    frame_data[0] = pci_len as u8; // SF PCI: 0x0N where N = payload length
    frame_data[1] = service_id;
    let copy_len = data.len().min(6); // max 6 data bytes in SF
    frame_data[2..2 + copy_len].copy_from_slice(&data[..copy_len]);
    CanFrame::new(profile.request_id, frame_data)
}

/// Build a ReadDataByIdentifier (0x22) request for a single DID.
pub fn build_read_did(profile: &EcuProfile, did: u16) -> CanFrame {
    let did_bytes = [(did >> 8) as u8, (did & 0xFF) as u8];
    build_uds_request(profile, 0x22, &did_bytes)
}

/// Build a ReadDTCInformation (0x19) request.
///
/// Sub-function 0x02 = reportDTCByStatusMask.
pub fn build_read_dtc_info(profile: &EcuProfile, sub_fn: u8, status_mask: u8) -> CanFrame {
    build_uds_request(profile, 0x19, &[sub_fn, status_mask])
}

/// Build a DiagnosticSessionControl (0x10) request.
pub fn build_session_control(profile: &EcuProfile, session_type: u8) -> CanFrame {
    build_uds_request(profile, 0x10, &[session_type])
}

/// Build a TesterPresent (0x3E) request with sub-function 0x00.
pub fn build_tester_present(profile: &EcuProfile) -> CanFrame {
    build_uds_request(profile, 0x3E, &[0x00])
}

// ── Send + receive ───────────────────────────────────────────────

/// Send a UDS request and collect the single-frame response.
///
/// Validates the service ID against the UDS safety allowlist before sending.
pub async fn uds_query(
    iface: &dyn CanInterface,
    profile: &EcuProfile,
    service_id: u8,
    data: &[u8],
    timeout: Duration,
) -> CanResult<Vec<u8>> {
    // Safety check
    if !uds_safety::is_uds_service_allowed(service_id) {
        return Err(CanError::UdsSafetyViolation {
            service_id,
            service_name: uds_safety::uds_service_name(service_id).to_string(),
        });
    }

    iface.drain_rx_buffer().await;

    let request = build_uds_request(profile, service_id, data);
    iface.send_frame(&request).await?;

    // Loop to skip unrelated CAN traffic until we get a frame from the
    // expected ECU response ID, or timeout expires.
    let deadline = std::time::Instant::now() + timeout;
    let response = loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(CanError::Timeout {
                timeout_ms: timeout.as_millis() as u64,
            });
        }
        let frame = iface.recv_frame(remaining).await?;
        if frame.id == profile.response_id {
            break frame;
        }
    };

    // Extract payload from ISO-TP single frame
    let pci_len = (response.data[0] & 0x0F) as usize;
    if pci_len == 0 || pci_len + 1 > response.data.len() {
        return Err(CanError::Protocol("invalid UDS response frame".into()));
    }

    let payload = &response.data[1..1 + pci_len];

    // Check for negative response
    if let Some((sid, nrc)) = is_negative_response(payload) {
        return Err(CanError::UdsNegativeResponse {
            service_id: sid,
            nrc,
            description: nrc_description(nrc).to_string(),
        });
    }

    // Verify positive response SID
    let expected_sid = service_id + POSITIVE_RESPONSE_OFFSET;
    if payload[0] != expected_sid {
        return Err(CanError::Protocol(format!(
            "expected positive response SID 0x{expected_sid:02X}, got 0x{:02X}",
            payload[0]
        )));
    }

    // Return payload after the positive response SID
    Ok(payload[1..].to_vec())
}

/// Send a UDS request and reassemble a multi-frame ISO-TP response.
///
/// Uses the ECU profile's CAN IDs instead of hardcoded OBD-II IDs.
pub async fn uds_query_isotp(
    iface: &dyn CanInterface,
    profile: &EcuProfile,
    service_id: u8,
    data: &[u8],
    timeout: Duration,
) -> CanResult<Vec<u8>> {
    // Safety check
    if !uds_safety::is_uds_service_allowed(service_id) {
        return Err(CanError::UdsSafetyViolation {
            service_id,
            service_name: uds_safety::uds_service_name(service_id).to_string(),
        });
    }

    iface.drain_rx_buffer().await;

    let request = build_uds_request(profile, service_id, data);
    iface.send_frame(&request).await?;

    // Loop to skip unrelated CAN traffic until we get a frame from the
    // expected ECU response ID, or timeout expires.
    let deadline = std::time::Instant::now() + timeout;
    let first = loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(CanError::Timeout {
                timeout_ms: timeout.as_millis() as u64,
            });
        }
        let frame = iface.recv_frame(remaining).await?;
        if frame.id == profile.response_id {
            break frame;
        }
    };

    let frame_type = (first.data[0] >> 4) & 0x0F;

    match frame_type {
        ISOTP_SF => {
            let len = (first.data[0] & 0x0F) as usize;
            if len == 0 || len + 1 > first.data.len() {
                return Err(CanError::IsoTp("invalid SF length".into()));
            }
            let payload = &first.data[1..1 + len];

            // Check for negative response
            if let Some((sid, nrc)) = is_negative_response(payload) {
                return Err(CanError::UdsNegativeResponse {
                    service_id: sid,
                    nrc,
                    description: nrc_description(nrc).to_string(),
                });
            }

            Ok(payload.to_vec())
        }
        ISOTP_FF => {
            let total_len = (((first.data[0] & 0x0F) as usize) << 8) | (first.data[1] as usize);
            let mut payload = Vec::with_capacity(total_len);
            let ff_data_end = first.data.len().min(8);
            payload.extend_from_slice(&first.data[2..ff_data_end]);

            // Send Flow Control using the ECU's request ID
            let fc_frame = CanFrame::new(
                profile.request_id,
                vec![0x30, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
            );
            iface.send_frame(&fc_frame).await?;

            let mut expected_seq = 1u8;
            while payload.len() < total_len {
                let cf = iface.recv_frame(timeout).await?;
                if cf.id != profile.response_id {
                    continue;
                }

                let cf_type = (cf.data[0] >> 4) & 0x0F;
                if cf_type != ISOTP_CF {
                    return Err(CanError::IsoTp(format!(
                        "expected CF (0x2), got 0x{cf_type:X}"
                    )));
                }

                let seq = cf.data[0] & 0x0F;
                if seq != (expected_seq & 0x0F) {
                    return Err(CanError::IsoTp(format!(
                        "sequence mismatch: expected {expected_seq}, got {seq}"
                    )));
                }

                let remaining = total_len - payload.len();
                let cf_data_end = cf.data.len().min(1 + remaining);
                payload.extend_from_slice(&cf.data[1..cf_data_end]);

                expected_seq = expected_seq.wrapping_add(1);
            }

            payload.truncate(total_len);

            // Check first bytes for negative response
            if let Some((sid, nrc)) = is_negative_response(&payload) {
                return Err(CanError::UdsNegativeResponse {
                    service_id: sid,
                    nrc,
                    description: nrc_description(nrc).to_string(),
                });
            }

            Ok(payload)
        }
        _ => Err(CanError::IsoTp(format!(
            "unexpected frame type 0x{frame_type:X}"
        ))),
    }
}

// ── Response helpers ─────────────────────────────────────────────

/// Check if a UDS payload is a negative response (SID = 0x7F).
///
/// Returns `Some((rejected_service_id, nrc))` if negative, `None` otherwise.
pub fn is_negative_response(data: &[u8]) -> Option<(u8, u8)> {
    if data.len() >= 3 && data[0] == NEGATIVE_RESPONSE_SID {
        Some((data[1], data[2]))
    } else {
        None
    }
}

/// Decode a UDS Negative Response Code (NRC) to a human-readable description.
pub fn nrc_description(nrc: u8) -> &'static str {
    match nrc {
        0x10 => "General reject",
        0x11 => "Service not supported",
        0x12 => "Sub-function not supported",
        0x13 => "Incorrect message length or invalid format",
        0x14 => "Response too long",
        0x21 => "Busy, repeat request",
        0x22 => "Conditions not correct",
        0x24 => "Request sequence error",
        0x25 => "No response from sub-net component",
        0x26 => "Failure prevents execution of requested action",
        0x31 => "Request out of range",
        0x33 => "Security access denied",
        0x35 => "Invalid key",
        0x36 => "Exceeded number of attempts",
        0x37 => "Required time delay not expired",
        0x70 => "Upload/download not accepted",
        0x71 => "Transfer data suspended",
        0x72 => "General programming failure",
        0x73 => "Wrong block sequence counter",
        0x78 => "Request correctly received, response pending",
        0x7E => "Sub-function not supported in active session",
        0x7F => "Service not supported in active session",
        _ => "Unknown NRC",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecu_profile::HELLA_BCR;
    use crate::mock::MockCanInterface;

    // ── Request builder tests ────────────────────────────────────

    #[test]
    fn build_read_did_frame() {
        let frame = build_read_did(&HELLA_BCR, 0xFD05);
        assert_eq!(frame.id, 0x60D);
        assert_eq!(frame.data[0], 0x03); // PCI: SF, length=3
        assert_eq!(frame.data[1], 0x22); // ReadDataByIdentifier
        assert_eq!(frame.data[2], 0xFD); // DID high byte
        assert_eq!(frame.data[3], 0x05); // DID low byte
    }

    #[test]
    fn build_read_dtc_info_frame() {
        let frame = build_read_dtc_info(&HELLA_BCR, 0x02, 0xFF);
        assert_eq!(frame.id, 0x60D);
        assert_eq!(frame.data[0], 0x03); // PCI: SF, length=3
        assert_eq!(frame.data[1], 0x19); // ReadDTCInformation
        assert_eq!(frame.data[2], 0x02); // sub-function
        assert_eq!(frame.data[3], 0xFF); // status mask
    }

    #[test]
    fn build_session_control_frame() {
        let frame = build_session_control(&HELLA_BCR, 0x03);
        assert_eq!(frame.id, 0x60D);
        assert_eq!(frame.data[0], 0x02); // PCI: SF, length=2
        assert_eq!(frame.data[1], 0x10); // DiagnosticSessionControl
        assert_eq!(frame.data[2], 0x03); // Extended session
    }

    #[test]
    fn build_tester_present_frame() {
        let frame = build_tester_present(&HELLA_BCR);
        assert_eq!(frame.id, 0x60D);
        assert_eq!(frame.data[0], 0x02); // PCI: SF, length=2
        assert_eq!(frame.data[1], 0x3E); // TesterPresent
        assert_eq!(frame.data[2], 0x00); // sub-function 0
    }

    #[test]
    fn build_generic_uds_request() {
        let frame = build_uds_request(&HELLA_BCR, 0x22, &[0xF1, 0x90]);
        assert_eq!(frame.id, 0x60D);
        assert_eq!(frame.data.len(), 8);
        assert_eq!(frame.data[0], 0x03); // length = 1 (SID) + 2 (data)
        assert_eq!(frame.data[1], 0x22);
        assert_eq!(frame.data[2], 0xF1);
        assert_eq!(frame.data[3], 0x90);
        // Remaining bytes padded with 0
        assert_eq!(frame.data[4], 0x00);
    }

    // ── Negative response detection ──────────────────────────────

    #[test]
    fn detect_negative_response() {
        let data = [0x7F, 0x22, 0x31]; // NRC 0x31 for service 0x22
        let (sid, nrc) = is_negative_response(&data).unwrap();
        assert_eq!(sid, 0x22);
        assert_eq!(nrc, 0x31);
    }

    #[test]
    fn positive_response_not_negative() {
        let data = [0x62, 0xFD, 0x05, 0x04, 0xCA]; // positive 0x22 response
        assert!(is_negative_response(&data).is_none());
    }

    #[test]
    fn short_data_not_negative() {
        assert!(is_negative_response(&[0x7F]).is_none());
        assert!(is_negative_response(&[0x7F, 0x22]).is_none());
    }

    // ── NRC descriptions ─────────────────────────────────────────

    #[test]
    fn nrc_known_codes() {
        assert_eq!(nrc_description(0x11), "Service not supported");
        assert_eq!(nrc_description(0x31), "Request out of range");
        assert_eq!(nrc_description(0x33), "Security access denied");
        assert_eq!(
            nrc_description(0x78),
            "Request correctly received, response pending"
        );
    }

    #[test]
    fn nrc_unknown_code() {
        assert_eq!(nrc_description(0xAA), "Unknown NRC");
    }

    // ── uds_query tests ──────────────────────────────────────────

    #[tokio::test]
    async fn uds_query_read_did_success() {
        let mock = MockCanInterface::new();
        // Positive response for ReadDataByIdentifier: 0x62 FD 05 04 CA
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x05, 0x62, 0xFD, 0x05, 0x04, 0xCA, 0x00, 0x00],
        ));

        let result = uds_query(&mock, &HELLA_BCR, 0x22, &[0xFD, 0x05], DEFAULT_UDS_TIMEOUT)
            .await
            .unwrap();

        // Should return payload after positive SID: [FD, 05, 04, CA]
        assert_eq!(result, vec![0xFD, 0x05, 0x04, 0xCA]);

        // Verify the request was sent correctly
        let sent = mock.sent_frames();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].id, 0x60D);
    }

    #[tokio::test]
    async fn uds_query_negative_response() {
        let mock = MockCanInterface::new();
        // Negative response: service 0x22, NRC 0x31 (request out of range)
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x03, 0x7F, 0x22, 0x31, 0x00, 0x00, 0x00, 0x00],
        ));

        let err = uds_query(&mock, &HELLA_BCR, 0x22, &[0xFD, 0x05], DEFAULT_UDS_TIMEOUT)
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            CanError::UdsNegativeResponse {
                service_id: 0x22,
                nrc: 0x31,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn uds_query_wrong_response_id_times_out() {
        let mock = MockCanInterface::new();
        // Response on wrong CAN ID — the recv loop skips it and times out
        mock.queue_response(CanFrame::new(
            0x7E8, // OBD-II response, not BCR
            vec![0x05, 0x62, 0xFD, 0x05, 0x04, 0xCA, 0x00, 0x00],
        ));

        let short_timeout = Duration::from_millis(100);
        let err = uds_query(&mock, &HELLA_BCR, 0x22, &[0xFD, 0x05], short_timeout)
            .await
            .unwrap_err();

        assert!(matches!(err, CanError::Timeout { .. }));
    }

    #[tokio::test]
    async fn uds_query_blocked_service() {
        let mock = MockCanInterface::new();

        let err = uds_query(
            &mock,
            &HELLA_BCR,
            0x2E, // WriteDataByIdentifier — blocked
            &[0xFD, 0x05, 0x00],
            DEFAULT_UDS_TIMEOUT,
        )
        .await
        .unwrap_err();

        assert!(matches!(err, CanError::UdsSafetyViolation { .. }));
        // No frame should have been sent
        assert!(mock.sent_frames().is_empty());
    }

    #[tokio::test]
    async fn uds_query_session_control() {
        let mock = MockCanInterface::new();
        // Positive response for DiagnosticSessionControl (extended session)
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x02, 0x50, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00],
        ));

        let result = uds_query(&mock, &HELLA_BCR, 0x10, &[0x03], DEFAULT_UDS_TIMEOUT)
            .await
            .unwrap();

        assert_eq!(result, vec![0x03]); // session type echoed back
    }

    #[tokio::test]
    async fn uds_query_tester_present() {
        let mock = MockCanInterface::new();
        // Positive response for TesterPresent
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x02, 0x7E, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        ));

        let result = uds_query(&mock, &HELLA_BCR, 0x3E, &[0x00], DEFAULT_UDS_TIMEOUT)
            .await
            .unwrap();

        assert_eq!(result, vec![0x00]); // sub-function echoed
    }

    // ── uds_query_isotp tests ────────────────────────────────────

    #[tokio::test]
    async fn uds_query_isotp_single_frame() {
        let mock = MockCanInterface::new();
        // SF positive response
        mock.queue_response(CanFrame::new(
            0x58D,
            vec![0x05, 0x62, 0xFD, 0x05, 0x04, 0xCA, 0x00, 0x00],
        ));

        let result = uds_query_isotp(&mock, &HELLA_BCR, 0x22, &[0xFD, 0x05], DEFAULT_UDS_TIMEOUT)
            .await
            .unwrap();

        // Returns full payload including positive response SID
        assert_eq!(result, vec![0x62, 0xFD, 0x05, 0x04, 0xCA]);
    }

    #[tokio::test]
    async fn uds_query_isotp_blocked_service() {
        let mock = MockCanInterface::new();

        let err = uds_query_isotp(
            &mock,
            &HELLA_BCR,
            0x27, // SecurityAccess — blocked
            &[0x01],
            DEFAULT_UDS_TIMEOUT,
        )
        .await
        .unwrap_err();

        assert!(matches!(err, CanError::UdsSafetyViolation { .. }));
    }
}
