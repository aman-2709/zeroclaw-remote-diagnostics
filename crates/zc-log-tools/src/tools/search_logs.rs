//! search_logs — regex search across log files with severity filtering.

use async_trait::async_trait;
use regex::Regex;
use serde_json::json;

use crate::error::{LogError, LogResult};
use crate::parsers;
use crate::source::LogSource;
use crate::types::{LogFormat, LogSeverity, LogTool, ToolResult};

pub struct SearchLogs;

#[async_trait]
impl LogTool for SearchLogs {
    fn name(&self) -> &str {
        "search_logs"
    }

    fn description(&self) -> &str {
        "Search log files with regex patterns and severity filtering"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the log file"
                },
                "query": {
                    "type": "string",
                    "description": "Regex pattern to search for"
                },
                "severity": {
                    "type": "string",
                    "enum": ["debug", "info", "notice", "warning", "error", "critical"],
                    "description": "Minimum severity level to include"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results (default: 100)",
                    "default": 100
                },
                "format": {
                    "type": "string",
                    "enum": ["syslog_3164", "syslog_5424", "journald", "json_lines", "plaintext"],
                    "description": "Log format (auto-detected if omitted)"
                }
            },
            "required": ["path", "query"]
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
        let query = args["query"]
            .as_str()
            .ok_or_else(|| LogError::Other("missing 'query' argument".into()))?;
        let limit = args["limit"].as_u64().unwrap_or(100) as usize;
        let min_severity = args["severity"]
            .as_str()
            .map(parse_severity_arg)
            .transpose()?;
        let format = args["format"].as_str().map(parse_format_arg).transpose()?;

        let re = Regex::new(query).map_err(|e| LogError::Regex(e.to_string()))?;

        let lines = source.read_lines(path).await?;
        let fmt = format.unwrap_or_else(|| parsers::detect_format(&lines));
        let entries = parsers::parse_lines(&lines, fmt);

        let matches: Vec<_> = entries
            .iter()
            .filter(|e| {
                if let Some(min) = min_severity
                    && e.severity < min
                {
                    return false;
                }
                re.is_match(&e.message) || re.is_match(&e.raw)
            })
            .take(limit)
            .map(|e| {
                json!({
                    "line": e.line_number,
                    "severity": e.severity.as_str(),
                    "message": e.message,
                    "timestamp": e.timestamp,
                    "source": e.source,
                })
            })
            .collect();

        let match_count = matches.len();
        let data = json!({
            "path": path,
            "query": query,
            "format": format!("{fmt:?}"),
            "total_lines": lines.len(),
            "matches": matches,
            "match_count": match_count,
        });

        Ok(ToolResult::success(
            "search_logs",
            data,
            format!("Found {match_count} matches for '{query}' in {path}"),
        ))
    }
}

fn parse_severity_arg(s: &str) -> LogResult<LogSeverity> {
    match s.to_lowercase().as_str() {
        "debug" => Ok(LogSeverity::Debug),
        "info" => Ok(LogSeverity::Info),
        "notice" => Ok(LogSeverity::Notice),
        "warning" | "warn" => Ok(LogSeverity::Warning),
        "error" => Ok(LogSeverity::Error),
        "critical" => Ok(LogSeverity::Critical),
        other => Err(LogError::Other(format!("unknown severity: {other}"))),
    }
}

fn parse_format_arg(s: &str) -> LogResult<LogFormat> {
    match s {
        "syslog_3164" => Ok(LogFormat::Syslog3164),
        "syslog_5424" => Ok(LogFormat::Syslog5424),
        "journald" => Ok(LogFormat::Journald),
        "json_lines" => Ok(LogFormat::JsonLines),
        "plaintext" => Ok(LogFormat::Plaintext),
        other => Err(LogError::Format(format!("unknown format: {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockLogSource;

    #[tokio::test]
    async fn search_by_pattern() {
        let source = MockLogSource::with_syslog_sample();
        let tool = SearchLogs;
        let result = tool
            .execute(
                json!({"path": "/var/log/syslog", "query": "database"}),
                &source,
            )
            .await
            .unwrap();
        assert!(result.success);
        let count = result.data.as_ref().unwrap()["match_count"]
            .as_u64()
            .unwrap();
        assert!(count >= 2, "should find 'database' in multiple lines");
    }

    #[tokio::test]
    async fn search_with_severity_filter() {
        let source = MockLogSource::with_json_sample();
        let tool = SearchLogs;
        let result = tool
            .execute(
                json!({"path": "/var/log/app.json", "query": ".*", "severity": "error"}),
                &source,
            )
            .await
            .unwrap();
        assert!(result.success);
        let matches = result.data.as_ref().unwrap()["matches"].as_array().unwrap();
        for m in matches {
            let sev = m["severity"].as_str().unwrap();
            assert!(
                sev == "error" || sev == "critical",
                "should only have error+, got {sev}"
            );
        }
    }

    #[tokio::test]
    async fn search_with_limit() {
        let source = MockLogSource::with_syslog_sample();
        let tool = SearchLogs;
        let result = tool
            .execute(
                json!({"path": "/var/log/syslog", "query": ".*", "limit": 3}),
                &source,
            )
            .await
            .unwrap();
        assert!(result.success);
        let count = result.data.as_ref().unwrap()["match_count"]
            .as_u64()
            .unwrap();
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn search_invalid_regex() {
        let source = MockLogSource::with_syslog_sample();
        let tool = SearchLogs;
        let result = tool
            .execute(
                json!({"path": "/var/log/syslog", "query": "[invalid"}),
                &source,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn search_missing_file() {
        let source = MockLogSource::new();
        let tool = SearchLogs;
        let result = tool
            .execute(json!({"path": "/nonexistent", "query": "test"}), &source)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn search_json_format() {
        let source = MockLogSource::with_json_sample();
        let tool = SearchLogs;
        let result = tool
            .execute(
                json!({"path": "/var/log/app.json", "query": "CAN bus"}),
                &source,
            )
            .await
            .unwrap();
        assert!(result.success);
        let count = result.data.as_ref().unwrap()["match_count"]
            .as_u64()
            .unwrap();
        assert!(count >= 2, "should find CAN bus entries");
    }
}
