//! OBD-II protocol helpers: request frame builders, response parsing,
//! ISO-TP multi-frame reassembly, and PID value decoders.

use std::time::Duration;

use crate::error::{CanError, CanResult};
use crate::interface::CanInterface;
use crate::types::*;

// ---------------------------------------------------------------------------
// Request builders
// ---------------------------------------------------------------------------

/// Build a standard OBD-II request frame for a given mode and PID.
pub fn build_request(mode: u8, pid: u8) -> CanFrame {
    CanFrame::new(
        OBD_REQUEST_ID,
        vec![0x02, mode, pid, 0x00, 0x00, 0x00, 0x00, 0x00],
    )
}

/// Build Mode 0x03 request (stored DTCs — no PID byte needed).
pub fn build_dtc_request() -> CanFrame {
    CanFrame::new(
        OBD_REQUEST_ID,
        vec![0x01, MODE_STORED_DTCS, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    )
}

// ---------------------------------------------------------------------------
// Send + receive helper
// ---------------------------------------------------------------------------

/// Default receive timeout for OBD-II responses.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(500);

/// Send an OBD-II request and collect the first ECU response.
pub async fn obd_query(
    iface: &dyn CanInterface,
    request: &CanFrame,
    timeout: Duration,
) -> CanResult<CanFrame> {
    iface.send_frame(request).await?;

    let response = iface.recv_frame(timeout).await?;
    if crate::interface::is_obd_response(response.id) {
        Ok(response)
    } else {
        Err(CanError::Protocol(format!(
            "expected OBD response ID 0x7E8-0x7EF, got 0x{:03X}",
            response.id
        )))
    }
}

// ---------------------------------------------------------------------------
// ISO-TP multi-frame reassembly (receive-only)
// ---------------------------------------------------------------------------

/// ISO-TP Flow Control frame: ContinueToSend, block_size=0, separation_time=0.
const FLOW_CONTROL_CTS: [u8; 8] = [0x30, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

/// Reassemble a multi-frame ISO-TP response.
///
/// Handles Single Frame, First Frame + Consecutive Frames.
/// Sends Flow Control after receiving First Frame.
pub async fn isotp_recv(
    iface: &dyn CanInterface,
    response_id: u32,
    timeout: Duration,
) -> CanResult<Vec<u8>> {
    let first = iface.recv_frame(timeout).await?;
    if first.id != response_id {
        return Err(CanError::Protocol(format!(
            "expected CAN ID 0x{response_id:03X}, got 0x{:03X}",
            first.id
        )));
    }

    let frame_type = (first.data[0] >> 4) & 0x0F;

    match frame_type {
        ISOTP_SF => {
            let len = (first.data[0] & 0x0F) as usize;
            if len == 0 || len + 1 > first.data.len() {
                return Err(CanError::IsoTp("invalid SF length".into()));
            }
            Ok(first.data[1..1 + len].to_vec())
        }
        ISOTP_FF => {
            let total_len = (((first.data[0] & 0x0F) as usize) << 8) | (first.data[1] as usize);

            let mut payload = Vec::with_capacity(total_len);
            let ff_data_end = first.data.len().min(8);
            payload.extend_from_slice(&first.data[2..ff_data_end]);

            // Send Flow Control
            let fc_frame = CanFrame::new(OBD_REQUEST_ID, FLOW_CONTROL_CTS.to_vec());
            iface.send_frame(&fc_frame).await?;

            let mut expected_seq = 1u8;
            while payload.len() < total_len {
                let cf = iface.recv_frame(timeout).await?;
                if cf.id != response_id {
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
            Ok(payload)
        }
        _ => Err(CanError::IsoTp(format!(
            "unexpected frame type 0x{frame_type:X}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// DTC byte decoding
// ---------------------------------------------------------------------------

/// Decode two raw bytes into a standard DTC code string (e.g., "P0300").
pub fn decode_dtc_bytes(b1: u8, b2: u8) -> Option<String> {
    if b1 == 0x00 && b2 == 0x00 {
        return None;
    }

    let category = match (b1 >> 6) & 0x03 {
        0 => 'P',
        1 => 'C',
        2 => 'B',
        3 => 'U',
        _ => unreachable!(),
    };

    let digit1 = (b1 >> 4) & 0x03;
    let digit2 = b1 & 0x0F;
    let digit3 = (b2 >> 4) & 0x0F;
    let digit4 = b2 & 0x0F;

    Some(format!("{category}{digit1}{digit2:X}{digit3:X}{digit4:X}"))
}

// ---------------------------------------------------------------------------
// PID value decoders
// ---------------------------------------------------------------------------

/// Decoded PID value with metadata.
#[derive(Debug, Clone)]
pub struct PidValue {
    pub pid: u8,
    pub name: &'static str,
    pub value: f64,
    pub unit: &'static str,
}

/// Decode a PID value from the raw data bytes (after SID and PID echo).
pub fn decode_pid(pid: u8, data_bytes: &[u8]) -> CanResult<PidValue> {
    let need = |n: usize| -> CanResult<()> {
        if data_bytes.len() < n {
            Err(CanError::Decode(format!(
                "PID 0x{pid:02X}: need {n} bytes, got {}",
                data_bytes.len()
            )))
        } else {
            Ok(())
        }
    };

    match pid {
        0x04 => {
            need(1)?;
            Ok(PidValue {
                pid,
                name: "Engine Load",
                value: data_bytes[0] as f64 * 100.0 / 255.0,
                unit: "%",
            })
        }
        0x05 => {
            need(1)?;
            Ok(PidValue {
                pid,
                name: "Coolant Temperature",
                value: data_bytes[0] as f64 - 40.0,
                unit: "°C",
            })
        }
        0x06 => {
            need(1)?;
            Ok(PidValue {
                pid,
                name: "Short Term Fuel Trim B1",
                value: (data_bytes[0] as f64 - 128.0) * 100.0 / 128.0,
                unit: "%",
            })
        }
        0x07 => {
            need(1)?;
            Ok(PidValue {
                pid,
                name: "Long Term Fuel Trim B1",
                value: (data_bytes[0] as f64 - 128.0) * 100.0 / 128.0,
                unit: "%",
            })
        }
        0x0B => {
            need(1)?;
            Ok(PidValue {
                pid,
                name: "Intake MAP",
                value: data_bytes[0] as f64,
                unit: "kPa",
            })
        }
        0x0C => {
            need(2)?;
            let rpm = ((data_bytes[0] as f64) * 256.0 + data_bytes[1] as f64) / 4.0;
            Ok(PidValue {
                pid,
                name: "Engine RPM",
                value: rpm,
                unit: "rpm",
            })
        }
        0x0D => {
            need(1)?;
            Ok(PidValue {
                pid,
                name: "Vehicle Speed",
                value: data_bytes[0] as f64,
                unit: "km/h",
            })
        }
        0x0E => {
            need(1)?;
            Ok(PidValue {
                pid,
                name: "Timing Advance",
                value: data_bytes[0] as f64 / 2.0 - 64.0,
                unit: "°",
            })
        }
        0x0F => {
            need(1)?;
            Ok(PidValue {
                pid,
                name: "Intake Air Temp",
                value: data_bytes[0] as f64 - 40.0,
                unit: "°C",
            })
        }
        0x10 => {
            need(2)?;
            let maf = ((data_bytes[0] as f64) * 256.0 + data_bytes[1] as f64) / 100.0;
            Ok(PidValue {
                pid,
                name: "MAF Rate",
                value: maf,
                unit: "g/s",
            })
        }
        0x11 => {
            need(1)?;
            Ok(PidValue {
                pid,
                name: "Throttle Position",
                value: data_bytes[0] as f64 * 100.0 / 255.0,
                unit: "%",
            })
        }
        0x1C => {
            need(1)?;
            Ok(PidValue {
                pid,
                name: "OBD Standard",
                value: data_bytes[0] as f64,
                unit: "",
            })
        }
        0x1F => {
            need(2)?;
            let secs = (data_bytes[0] as f64) * 256.0 + data_bytes[1] as f64;
            Ok(PidValue {
                pid,
                name: "Runtime Since Start",
                value: secs,
                unit: "s",
            })
        }
        0x2F => {
            need(1)?;
            Ok(PidValue {
                pid,
                name: "Fuel Level",
                value: data_bytes[0] as f64 * 100.0 / 255.0,
                unit: "%",
            })
        }
        0x33 => {
            need(1)?;
            Ok(PidValue {
                pid,
                name: "Barometric Pressure",
                value: data_bytes[0] as f64,
                unit: "kPa",
            })
        }
        0x42 => {
            need(2)?;
            let v = ((data_bytes[0] as f64) * 256.0 + data_bytes[1] as f64) / 1000.0;
            Ok(PidValue {
                pid,
                name: "Control Module Voltage",
                value: v,
                unit: "V",
            })
        }
        0x46 => {
            need(1)?;
            Ok(PidValue {
                pid,
                name: "Ambient Air Temp",
                value: data_bytes[0] as f64 - 40.0,
                unit: "°C",
            })
        }
        0x49 => {
            need(1)?;
            Ok(PidValue {
                pid,
                name: "Accel Pedal D",
                value: data_bytes[0] as f64 * 100.0 / 255.0,
                unit: "%",
            })
        }
        0x4C => {
            need(1)?;
            Ok(PidValue {
                pid,
                name: "Cmd Throttle Actuator",
                value: data_bytes[0] as f64 * 100.0 / 255.0,
                unit: "%",
            })
        }
        0x51 => {
            need(1)?;
            Ok(PidValue {
                pid,
                name: "Fuel Type",
                value: data_bytes[0] as f64,
                unit: "",
            })
        }
        _ => Err(CanError::UnknownPid { pid }),
    }
}

/// Parse a standard OBD-II single-frame response for Mode 0x01/0x02.
///
/// Expected frame data layout: `[num_bytes, response_sid, pid, data...]`
/// Returns `(pid, data_bytes_slice)`.
pub fn parse_pid_response(frame: &CanFrame, expected_mode: u8) -> CanResult<(u8, &[u8])> {
    if frame.data.len() < 3 {
        return Err(CanError::Protocol("response too short".into()));
    }

    let expected_sid = expected_mode + RESPONSE_SID_OFFSET;
    let sid = frame.data[1];
    if sid != expected_sid {
        return Err(CanError::Protocol(format!(
            "expected SID 0x{expected_sid:02X}, got 0x{sid:02X}"
        )));
    }

    let pid = frame.data[2];
    let num_bytes = frame.data[0] as usize;
    let data_len = num_bytes.saturating_sub(2);
    let data_end = (3 + data_len).min(frame.data.len());
    Ok((pid, &frame.data[3..data_end]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_mode01() {
        let frame = build_request(MODE_CURRENT_DATA, 0x0C);
        assert_eq!(frame.id, OBD_REQUEST_ID);
        assert_eq!(frame.data[0], 0x02);
        assert_eq!(frame.data[1], 0x01);
        assert_eq!(frame.data[2], 0x0C);
    }

    #[test]
    fn build_dtc_request_mode03() {
        let frame = build_dtc_request();
        assert_eq!(frame.data[0], 0x01);
        assert_eq!(frame.data[1], MODE_STORED_DTCS);
    }

    // --- DTC byte decoding ---

    #[test]
    fn decode_dtc_p0300() {
        assert_eq!(decode_dtc_bytes(0x03, 0x00).as_deref(), Some("P0300"));
    }

    #[test]
    fn decode_dtc_p0171() {
        assert_eq!(decode_dtc_bytes(0x01, 0x71).as_deref(), Some("P0171"));
    }

    #[test]
    fn decode_dtc_c0035() {
        assert_eq!(decode_dtc_bytes(0x40, 0x35).as_deref(), Some("C0035"));
    }

    #[test]
    fn decode_dtc_u0100() {
        assert_eq!(decode_dtc_bytes(0xC1, 0x00).as_deref(), Some("U0100"));
    }

    #[test]
    fn decode_dtc_padding_returns_none() {
        assert_eq!(decode_dtc_bytes(0x00, 0x00), None);
    }

    // --- PID decoding ---

    #[test]
    fn decode_pid_rpm() {
        let v = decode_pid(0x0C, &[0x36, 0xB0]).unwrap();
        assert_eq!(v.name, "Engine RPM");
        assert!((v.value - 3500.0).abs() < 0.01);
        assert_eq!(v.unit, "rpm");
    }

    #[test]
    fn decode_pid_speed() {
        let v = decode_pid(0x0D, &[0x3C]).unwrap();
        assert_eq!(v.name, "Vehicle Speed");
        assert!((v.value - 60.0).abs() < 0.01);
    }

    #[test]
    fn decode_pid_coolant_temp() {
        let v = decode_pid(0x05, &[130]).unwrap();
        assert!((v.value - 90.0).abs() < 0.01);
    }

    #[test]
    fn decode_pid_engine_load() {
        let v = decode_pid(0x04, &[127]).unwrap();
        assert!((v.value - 49.803).abs() < 0.01);
    }

    #[test]
    fn decode_pid_fuel_level() {
        let v = decode_pid(0x2F, &[128]).unwrap();
        assert!((v.value - 50.196).abs() < 0.01);
    }

    #[test]
    fn decode_pid_throttle() {
        let v = decode_pid(0x11, &[255]).unwrap();
        assert!((v.value - 100.0).abs() < 0.01);
    }

    #[test]
    fn decode_pid_unsupported() {
        let err = decode_pid(0xFF, &[0x00]).unwrap_err();
        assert!(matches!(err, CanError::UnknownPid { pid: 0xFF }));
    }

    #[test]
    fn decode_pid_insufficient_bytes() {
        let err = decode_pid(0x0C, &[0x36]).unwrap_err();
        assert!(matches!(err, CanError::Decode(_)));
    }

    // --- Response parsing ---

    #[test]
    fn parse_pid_response_mode01() {
        let frame = CanFrame::new(0x7E8, vec![0x04, 0x41, 0x0C, 0x1B, 0x58, 0x00, 0x00, 0x00]);
        let (pid, data) = parse_pid_response(&frame, MODE_CURRENT_DATA).unwrap();
        assert_eq!(pid, 0x0C);
        assert_eq!(data, &[0x1B, 0x58]);
    }

    #[test]
    fn parse_pid_response_wrong_sid() {
        let frame = CanFrame::new(0x7E8, vec![0x04, 0x42, 0x0C, 0x1B, 0x58, 0x00, 0x00, 0x00]);
        let err = parse_pid_response(&frame, MODE_CURRENT_DATA).unwrap_err();
        assert!(matches!(err, CanError::Protocol(_)));
    }

    // --- ISO-TP ---

    #[tokio::test]
    async fn isotp_single_frame() {
        let mock = crate::mock::MockCanInterface::new();
        mock.queue_response(CanFrame::new(
            0x7E8,
            vec![0x05, 0x49, 0x02, 0x01, 0x57, 0x42],
        ));

        let data = isotp_recv(&mock, 0x7E8, DEFAULT_TIMEOUT).await.unwrap();
        assert_eq!(data.len(), 5);
        assert_eq!(data[0], 0x49);
    }

    #[tokio::test]
    async fn isotp_multi_frame_vin() {
        let mock = crate::mock::MockCanInterface::new();

        // First Frame: total_len=20
        mock.queue_response(CanFrame::new(
            0x7E8,
            vec![0x10, 0x14, 0x49, 0x02, 0x01, 0x57, 0x44, 0x42],
        ));
        // CF 1
        mock.queue_response(CanFrame::new(
            0x7E8,
            vec![0x21, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47],
        ));
        // CF 2
        mock.queue_response(CanFrame::new(
            0x7E8,
            vec![0x22, 0x48, 0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4E],
        ));

        let data = isotp_recv(&mock, 0x7E8, DEFAULT_TIMEOUT).await.unwrap();
        assert_eq!(data.len(), 20);
    }
}
