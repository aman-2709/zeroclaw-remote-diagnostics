//! Mock log source for testing — serves pre-loaded log content.

use async_trait::async_trait;
use std::collections::HashMap;

use crate::error::{LogError, LogResult};
use crate::source::LogSource;

/// A mock log source that serves pre-loaded content by path.
pub struct MockLogSource {
    files: HashMap<String, Vec<String>>,
}

impl MockLogSource {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    /// Add a file with the given lines.
    pub fn add_file(&mut self, path: impl Into<String>, lines: Vec<String>) {
        self.files.insert(path.into(), lines);
    }

    /// Create a mock with a sample syslog (RFC 3164) file.
    pub fn with_syslog_sample() -> Self {
        let mut m = Self::new();
        m.add_file(
            "/var/log/syslog",
            vec![
                "<134>Jan 15 12:00:01 edge1 myapp[1234]: Service started successfully".into(),
                "<131>Jan 15 12:00:05 edge1 myapp[1234]: Failed to connect to database: connection refused".into(),
                "<134>Jan 15 12:00:10 edge1 kernel: [12345.678] eth0: link up".into(),
                "<132>Jan 15 12:00:15 edge1 myapp[1234]: Critical: disk usage at 95%".into(),
                "<134>Jan 15 12:00:20 edge1 cron[5678]: (root) CMD (/usr/bin/healthcheck)".into(),
                "<133>Jan 15 12:00:25 edge1 myapp[1234]: Error reading CAN bus: timeout after 500ms".into(),
                "<134>Jan 15 12:00:30 edge1 systemd[1]: Started Daily Cleanup".into(),
                "<131>Jan 15 12:00:35 edge1 myapp[1234]: Failed to connect to database: connection refused".into(),
                "<134>Jan 15 12:00:40 edge1 myapp[1234]: Retrying connection in 5 seconds".into(),
                "<134>Jan 15 12:00:45 edge1 myapp[1234]: Database connection established".into(),
            ],
        );
        m
    }

    /// Create a mock with a sample JSON lines file.
    pub fn with_json_sample() -> Self {
        let mut m = Self::new();
        m.add_file(
            "/var/log/app.json",
            vec![
                r#"{"timestamp":"2024-01-15T12:00:01Z","level":"info","message":"Service started","service":"zeroclaw-agent"}"#.into(),
                r#"{"timestamp":"2024-01-15T12:00:05Z","level":"error","message":"CAN bus timeout","service":"canbus","error_code":"E001"}"#.into(),
                r#"{"timestamp":"2024-01-15T12:00:10Z","level":"warning","message":"Memory usage high: 82%","service":"monitor"}"#.into(),
                r#"{"timestamp":"2024-01-15T12:00:15Z","level":"info","message":"OBD query completed: RPM=2500","service":"canbus"}"#.into(),
                r#"{"timestamp":"2024-01-15T12:00:20Z","level":"error","message":"Connection refused: MQTT broker","service":"mqtt","retry":3}"#.into(),
                r#"{"timestamp":"2024-01-15T12:00:25Z","level":"debug","message":"Heartbeat sent","service":"zeroclaw-agent"}"#.into(),
                r#"{"timestamp":"2024-01-15T12:00:30Z","level":"error","message":"CAN bus timeout","service":"canbus","error_code":"E001"}"#.into(),
                r#"{"timestamp":"2024-01-15T12:00:35Z","level":"info","message":"DTC scan complete: 2 codes found","service":"canbus"}"#.into(),
            ],
        );
        m
    }

    /// Create a mock with a sample journald export file.
    pub fn with_journald_sample() -> Self {
        let mut m = Self::new();
        m.add_file(
            "/var/log/journal.export",
            vec![
                "__REALTIME_TIMESTAMP=1705312801000000".into(),
                "_HOSTNAME=edge1".into(),
                "SYSLOG_IDENTIFIER=zeroclaw".into(),
                "PRIORITY=6".into(),
                "MESSAGE=Agent started".into(),
                "".into(),
                "__REALTIME_TIMESTAMP=1705312805000000".into(),
                "_HOSTNAME=edge1".into(),
                "SYSLOG_IDENTIFIER=zeroclaw".into(),
                "PRIORITY=3".into(),
                "MESSAGE=CAN interface error: device not found".into(),
                "".into(),
                "__REALTIME_TIMESTAMP=1705312810000000".into(),
                "_HOSTNAME=edge1".into(),
                "SYSLOG_IDENTIFIER=kernel".into(),
                "PRIORITY=4".into(),
                "MESSAGE=can0: bus-off recovery".into(),
                "".into(),
            ],
        );
        m
    }

    /// Create a mock with a sample plaintext log file.
    pub fn with_plaintext_sample() -> Self {
        let mut m = Self::new();
        m.add_file(
            "/var/log/app.log",
            vec![
                "2024-01-15 12:00:01 INFO Starting application".into(),
                "2024-01-15 12:00:05 ERROR Failed to open CAN interface: permission denied".into(),
                "2024-01-15 12:00:10 WARNING Low disk space on /data".into(),
                "2024-01-15 12:00:15 DEBUG Checking OBD connection".into(),
                "2024-01-15 12:00:20 ERROR Segmentation fault in module canbus".into(),
                "2024-01-15 12:00:25 CRITICAL System temperature exceeds threshold".into(),
                "2024-01-15 12:00:30 INFO Restarting canbus service".into(),
            ],
        );
        m
    }
}

impl Default for MockLogSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LogSource for MockLogSource {
    async fn read_lines(&self, path: &str) -> LogResult<Vec<String>> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| LogError::NotFound(path.to_string()))
    }

    async fn tail_lines(&self, path: &str, count: usize) -> LogResult<Vec<String>> {
        let lines = self.read_lines(path).await?;
        let start = lines.len().saturating_sub(count);
        Ok(lines[start..].to_vec())
    }

    async fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    async fn list_sources(&self) -> LogResult<Vec<String>> {
        Ok(self.files.keys().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_read_lines() {
        let source = MockLogSource::with_syslog_sample();
        let lines = source.read_lines("/var/log/syslog").await.unwrap();
        assert_eq!(lines.len(), 10);
    }

    #[tokio::test]
    async fn mock_tail_lines() {
        let source = MockLogSource::with_syslog_sample();
        let lines = source.tail_lines("/var/log/syslog", 3).await.unwrap();
        assert_eq!(lines.len(), 3);
        assert!(lines[2].contains("Database connection established"));
    }

    #[tokio::test]
    async fn mock_not_found() {
        let source = MockLogSource::new();
        let result = source.read_lines("/nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn mock_exists() {
        let source = MockLogSource::with_json_sample();
        assert!(source.exists("/var/log/app.json").await);
        assert!(!source.exists("/nonexistent").await);
    }
}
