use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single telemetry reading from a device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryReading {
    /// Device that produced this reading.
    pub device_id: String,
    /// Timestamp of the reading.
    pub time: DateTime<Utc>,
    /// Metric name (e.g., "engine_rpm", "coolant_temp", "cpu_usage").
    pub metric_name: String,
    /// Numeric value (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_numeric: Option<f64>,
    /// Text value (if applicable, e.g., DTC code string).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_text: Option<String>,
    /// Complex/structured value as JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_json: Option<serde_json::Value>,
    /// Unit of measurement (e.g., "rpm", "celsius", "percent").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Source subsystem (e.g., "obd2", "system", "canbus").
    pub source: TelemetrySource,
}

/// Source subsystem for telemetry data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetrySource {
    Obd2,
    System,
    Canbus,
}

/// OBD-II sensor data from a specific PID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorData {
    /// OBD-II PID (e.g., 0x0C for engine RPM).
    pub pid: u8,
    /// Human-readable name.
    pub name: String,
    /// Decoded numeric value.
    pub value: f64,
    /// Unit of measurement.
    pub unit: String,
    /// Raw bytes from the ECU response.
    #[serde(with = "hex_bytes")]
    pub raw_bytes: Vec<u8>,
}

/// System metrics from the edge device itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetrics {
    /// CPU usage percentage (0.0 - 100.0).
    pub cpu_percent: f64,
    /// Memory usage in bytes.
    pub memory_used_bytes: u64,
    /// Total memory in bytes.
    pub memory_total_bytes: u64,
    /// Disk usage percentage.
    pub disk_percent: f64,
    /// Device uptime in seconds.
    pub uptime_secs: u64,
    /// CPU temperature in celsius (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_temp_celsius: Option<f64>,
    /// Ollama process running.
    pub ollama_running: bool,
    /// CAN interface up.
    pub can_interface_up: bool,
}

/// Batch of telemetry readings for efficient MQTT publishing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryBatch {
    pub device_id: String,
    pub readings: Vec<TelemetryReading>,
    pub collected_at: DateTime<Utc>,
}

mod hex_bytes {
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex_string: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        serializer.serialize_str(&hex_string)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(serde::de::Error::custom))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_reading_roundtrip() {
        let reading = TelemetryReading {
            device_id: "rpi-001".into(),
            time: Utc::now(),
            metric_name: "engine_rpm".into(),
            value_numeric: Some(3500.0),
            value_text: None,
            value_json: None,
            unit: Some("rpm".into()),
            source: TelemetrySource::Obd2,
        };
        let json = serde_json::to_string(&reading).unwrap();
        let deserialized: TelemetryReading = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.metric_name, "engine_rpm");
        assert_eq!(deserialized.value_numeric, Some(3500.0));
    }

    #[test]
    fn sensor_data_hex_serialization() {
        let sensor = SensorData {
            pid: 0x0C,
            name: "Engine RPM".into(),
            value: 3500.0,
            unit: "rpm".into(),
            raw_bytes: vec![0x41, 0x0C, 0x1B, 0x58],
        };
        let json = serde_json::to_string(&sensor).unwrap();
        assert!(json.contains("410c1b58"));

        let deserialized: SensorData = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.raw_bytes, vec![0x41, 0x0C, 0x1B, 0x58]);
    }

    #[test]
    fn system_metrics_roundtrip() {
        let metrics = SystemMetrics {
            cpu_percent: 45.2,
            memory_used_bytes: 512_000_000,
            memory_total_bytes: 4_000_000_000,
            disk_percent: 30.0,
            uptime_secs: 86400,
            cpu_temp_celsius: Some(55.3),
            ollama_running: true,
            can_interface_up: true,
        };
        let json = serde_json::to_string(&metrics).unwrap();
        let deserialized: SystemMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.cpu_percent, 45.2);
        assert!(deserialized.ollama_running);
    }

    #[test]
    fn telemetry_source_serialization() {
        assert_eq!(
            serde_json::to_string(&TelemetrySource::Obd2).unwrap(),
            r#""obd2""#
        );
        assert_eq!(
            serde_json::to_string(&TelemetrySource::System).unwrap(),
            r#""system""#
        );
    }
}
