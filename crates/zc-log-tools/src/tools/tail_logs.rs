//! tail_logs — show the last N log entries with optional severity filtering.

use async_trait::async_trait;
use serde_json::json;

use crate::error::{LogError, LogResult};
use crate::parsers;
use crate::source::LogSource;
use crate::types::{LogFormat, LogSeverity, LogTool, ToolResult};

pub struct TailLogs;

#[async_trait]
impl LogTool for TailLogs {
    fn name(&self) -> &str {
        "tail_logs"
    }

    fn description(&self) -> &str {
        "Show the last N log entries with optional severity filtering"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the log file"
                },
                "count": {
                    "type": "integer",
                    "description": "Number of entries to show (default: 50)",
                    "default": 50
                },
                "severity": {
                    "type": "string",
                    "enum": ["debug", "info", "notice", "warning", "error", "critical"],
                    "description": "Minimum severity level to include"
                },
                "format": {
                    "type": "string",
                    "enum": ["syslog_3164", "syslog_5424", "journald", "json_lines", "plaintext"],
                    "description": "Log format (auto-detected if omitted)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        source: &dyn LogSource,
    ) -> LogResult<ToolResult> {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| LogError::Other("missing 'path' argument".into()))?;
        let count = args["count"].as_u64().unwrap_or(50) as usize;
        let min_severity = args["severity"]
            .as_str()
            .map(|s| match s.to_lowercase().as_str() {
                "debug" => Ok(LogSeverity::Debug),
                "info" => Ok(LogSeverity::Info),
                "notice" => Ok(LogSeverity::Notice),
                "warning" | "warn" => Ok(LogSeverity::Warning),
                "error" => Ok(LogSeverity::Error),
                "critical" => Ok(LogSeverity::Critical),
                other => Err(LogError::Other(format!("unknown severity: {other}"))),
            })
            .transpose()?;
        let format = args["format"]
            .as_str()
            .map(|s| match s {
                "syslog_3164" => Ok(LogFormat::Syslog3164),
                "syslog_5424" => Ok(LogFormat::Syslog5424),
                "journald" => Ok(LogFormat::Journald),
                "json_lines" => Ok(LogFormat::JsonLines),
                "plaintext" => Ok(LogFormat::Plaintext),
                other => Err(LogError::Format(format!("unknown format: {other}"))),
            })
            .transpose()?;

        // Read all lines — needed for multi-line formats (journald) and
        // severity filtering (can't know how many raw lines to fetch)
        let lines = source.read_lines(path).await?;
        let fmt = format.unwrap_or_else(|| parsers::detect_format(&lines));
        let entries = parsers::parse_lines(&lines, fmt);

        // Apply severity filter
        let filtered: Vec<_> = if let Some(min) = min_severity {
            entries.iter().filter(|e| e.severity >= min).collect()
        } else {
            entries.iter().collect()
        };

        // Take the last `count` entries
        let start = filtered.len().saturating_sub(count);
        let tail = &filtered[start..];

        let data = json!({
            "path": path,
            "format": format!("{fmt:?}"),
            "total_entries": entries.len(),
            "filtered_entries": filtered.len(),
            "shown": tail.len(),
            "entries": tail.iter().map(|e| json!({
                "line": e.line_number,
                "severity": e.severity.as_str(),
                "message": e.message,
                "timestamp": e.timestamp,
                "source": e.source,
            })).collect::<Vec<_>>(),
        });

        let shown = tail.len();
        Ok(ToolResult::success(
            "tail_logs",
            data,
            format!("Showing last {shown} entries from {path}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockLogSource;

    #[tokio::test]
    async fn tail_syslog() {
        let source = MockLogSource::with_syslog_sample();
        let tool = TailLogs;
        let result = tool
            .execute(json!({"path": "/var/log/syslog", "count": 3}), &source)
            .await
            .unwrap();
        assert!(result.success);
        let data = result.data.as_ref().unwrap();
        assert_eq!(data["shown"].as_u64().unwrap(), 3);
    }

    #[tokio::test]
    async fn tail_with_severity_filter() {
        let source = MockLogSource::with_syslog_sample();
        let tool = TailLogs;
        let result = tool
            .execute(
                json!({"path": "/var/log/syslog", "count": 50, "severity": "error"}),
                &source,
            )
            .await
            .unwrap();
        assert!(result.success);
        let entries = result.data.as_ref().unwrap()["entries"].as_array().unwrap();
        for entry in entries {
            let sev = entry["severity"].as_str().unwrap();
            assert!(
                sev == "error" || sev == "critical",
                "should only show error+, got {sev}"
            );
        }
    }

    #[tokio::test]
    async fn tail_json_logs() {
        let source = MockLogSource::with_json_sample();
        let tool = TailLogs;
        let result = tool
            .execute(json!({"path": "/var/log/app.json", "count": 5}), &source)
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.data.as_ref().unwrap()["shown"].as_u64().unwrap(), 5);
    }

    #[tokio::test]
    async fn tail_journald() {
        let source = MockLogSource::with_journald_sample();
        let tool = TailLogs;
        let result = tool
            .execute(
                json!({"path": "/var/log/journal.export", "count": 2}),
                &source,
            )
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.data.as_ref().unwrap()["shown"].as_u64().unwrap(), 2);
    }

    #[tokio::test]
    async fn tail_more_than_available() {
        let source = MockLogSource::with_json_sample();
        let tool = TailLogs;
        let result = tool
            .execute(json!({"path": "/var/log/app.json", "count": 1000}), &source)
            .await
            .unwrap();
        assert!(result.success);
        let data = result.data.as_ref().unwrap();
        // Should return all entries, not error
        assert_eq!(data["shown"], data["total_entries"]);
    }

    #[tokio::test]
    async fn tail_plaintext() {
        let source = MockLogSource::with_plaintext_sample();
        let tool = TailLogs;
        let result = tool
            .execute(json!({"path": "/var/log/app.log", "count": 3}), &source)
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.data.as_ref().unwrap()["shown"].as_u64().unwrap(), 3);
    }

    #[tokio::test]
    async fn tail_missing_file() {
        let source = MockLogSource::new();
        let tool = TailLogs;
        let result = tool
            .execute(json!({"path": "/nonexistent", "count": 10}), &source)
            .await;
        assert!(result.is_err());
    }
}
