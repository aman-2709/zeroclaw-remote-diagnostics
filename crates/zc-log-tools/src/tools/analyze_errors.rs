//! analyze_errors — detect and classify error patterns in log files.

use async_trait::async_trait;
use regex::Regex;
use serde_json::json;
use std::collections::HashMap;
use std::sync::LazyLock;

use crate::error::{LogError, LogResult};
use crate::parsers;
use crate::source::LogSource;
use crate::types::{LogFormat, LogSeverity, LogTool, ToolResult};

// ── Known error pattern categories ────────────────────────────

struct ErrorPattern {
    category: &'static str,
    regex: Regex,
    description: &'static str,
}

impl ErrorPattern {
    fn new(category: &'static str, pattern: &str, description: &'static str) -> Self {
        Self {
            category,
            regex: Regex::new(pattern).unwrap(),
            description,
        }
    }
}

static ERROR_PATTERNS: LazyLock<Vec<ErrorPattern>> = LazyLock::new(|| {
    vec![
        ErrorPattern::new(
            "connection_error",
            r"(?i)(connection\s+(refused|reset|timed?\s*out|closed)|ECONNREFUSED|ECONNRESET|ETIMEDOUT)",
            "Network connectivity issue",
        ),
        ErrorPattern::new(
            "permission_error",
            r"(?i)(permission\s+denied|access\s+denied|unauthorized|forbidden|EACCES)",
            "Permission or access control issue",
        ),
        ErrorPattern::new(
            "resource_error",
            r"(?i)(out\s+of\s+memory|OOM|disk\s+full|no\s+space|ENOMEM|ENOSPC)",
            "Resource exhaustion",
        ),
        ErrorPattern::new(
            "service_error",
            r"(?i)(service\s+(unavailable|stopped|failed)|failed\s+to\s+start)",
            "Service lifecycle issue",
        ),
        ErrorPattern::new(
            "file_error",
            r"(?i)(file\s+not\s+found|no\s+such\s+file|ENOENT|not\s+found)",
            "File or path not found",
        ),
        ErrorPattern::new(
            "dns_error",
            r"(?i)(DNS\s+(resolution|lookup)\s+failed|host\s+not\s+found|NXDOMAIN)",
            "DNS resolution failure",
        ),
        ErrorPattern::new(
            "process_error",
            r"(?i)(segfault|segmentation\s+fault|core\s+dumped|signal\s+(11|SIGSEGV|SIGKILL|SIGABRT))",
            "Process crash or signal",
        ),
        ErrorPattern::new(
            "timeout_error",
            r"(?i)(timeout|timed?\s*out|deadline\s+exceeded)",
            "Operation timeout",
        ),
        ErrorPattern::new(
            "can_bus_error",
            r"(?i)(CAN\s+(bus|interface)\s+(error|timeout|offline)|bus.off|can\d+:\s+)",
            "CAN bus communication issue",
        ),
    ]
});

// ── Tool implementation ───────────────────────────────────────

pub struct AnalyzeErrors;

#[async_trait]
impl LogTool for AnalyzeErrors {
    fn name(&self) -> &str {
        "analyze_errors"
    }

    fn description(&self) -> &str {
        "Detect and classify error patterns in log files"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the log file"
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

        let lines = source.read_lines(path).await?;
        let fmt = format.unwrap_or_else(|| parsers::detect_format(&lines));
        let entries = parsers::parse_lines(&lines, fmt);

        // Filter to error/critical entries
        let errors: Vec<_> = entries
            .iter()
            .filter(|e| e.severity >= LogSeverity::Error)
            .collect();

        // Classify by pattern
        let mut categories: HashMap<&str, CategoryStats> = HashMap::new();
        let mut unclassified_count = 0;
        let mut unclassified_examples = Vec::new();

        for entry in &errors {
            let mut classified = false;
            for pattern in ERROR_PATTERNS.iter() {
                if pattern.regex.is_match(&entry.message) || pattern.regex.is_match(&entry.raw) {
                    let stats = categories
                        .entry(pattern.category)
                        .or_insert_with(|| CategoryStats::new(pattern.description));
                    stats.count += 1;
                    if stats.examples.len() < 3 {
                        stats.examples.push(entry.message.clone());
                    }
                    if stats.first_seen.is_none() {
                        stats.first_seen = entry.timestamp;
                    }
                    stats.last_seen = entry.timestamp;
                    classified = true;
                    break;
                }
            }
            if !classified {
                unclassified_count += 1;
                if unclassified_examples.len() < 5 {
                    unclassified_examples.push(json!({
                        "line": entry.line_number,
                        "message": entry.message,
                        "severity": entry.severity.as_str(),
                        "timestamp": entry.timestamp,
                    }));
                }
            }
        }

        let total_lines = entries.len();
        let error_count = errors.len();
        let warning_count = entries
            .iter()
            .filter(|e| e.severity == LogSeverity::Warning)
            .count();

        // Sort patterns by count (descending)
        let mut patterns: Vec<_> = categories
            .iter()
            .map(|(cat, stats)| {
                json!({
                    "category": cat,
                    "description": stats.description,
                    "count": stats.count,
                    "first_seen": stats.first_seen,
                    "last_seen": stats.last_seen,
                    "examples": stats.examples,
                })
            })
            .collect();
        patterns.sort_by(|a, b| {
            b["count"]
                .as_u64()
                .unwrap_or(0)
                .cmp(&a["count"].as_u64().unwrap_or(0))
        });

        let classified_count = error_count - unclassified_count;
        let classification_rate = if error_count > 0 {
            (classified_count as f64 / error_count as f64 * 100.0).round()
        } else {
            100.0
        };

        let data = json!({
            "path": path,
            "format": format!("{fmt:?}"),
            "total_lines": total_lines,
            "error_count": error_count,
            "warning_count": warning_count,
            "patterns": patterns,
            "unclassified_count": unclassified_count,
            "unclassified_examples": unclassified_examples,
            "classification_rate": classification_rate,
        });

        let pattern_count = categories.len();
        Ok(ToolResult::success(
            "analyze_errors",
            data,
            format!(
                "Found {error_count} errors ({pattern_count} patterns, {classification_rate}% classified) and {warning_count} warnings in {total_lines} log lines"
            ),
        ))
    }
}

struct CategoryStats {
    description: &'static str,
    count: usize,
    examples: Vec<String>,
    first_seen: Option<chrono::DateTime<chrono::Utc>>,
    last_seen: Option<chrono::DateTime<chrono::Utc>>,
}

impl CategoryStats {
    fn new(description: &'static str) -> Self {
        Self {
            description,
            count: 0,
            examples: Vec::new(),
            first_seen: None,
            last_seen: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockLogSource;

    #[tokio::test]
    async fn analyze_syslog_errors() {
        let source = MockLogSource::with_syslog_sample();
        let tool = AnalyzeErrors;
        let result = tool
            .execute(json!({"path": "/var/log/syslog"}), &source)
            .await
            .unwrap();
        assert!(result.success);
        let data = result.data.as_ref().unwrap();
        let error_count = data["error_count"].as_u64().unwrap();
        assert!(error_count >= 2, "should detect errors");
        let patterns = data["patterns"].as_array().unwrap();
        assert!(!patterns.is_empty(), "should classify patterns");
    }

    #[tokio::test]
    async fn analyze_json_errors() {
        let source = MockLogSource::with_json_sample();
        let tool = AnalyzeErrors;
        let result = tool
            .execute(json!({"path": "/var/log/app.json"}), &source)
            .await
            .unwrap();
        assert!(result.success);
        let data = result.data.as_ref().unwrap();
        assert!(data["error_count"].as_u64().unwrap() >= 2);
    }

    #[tokio::test]
    async fn analyze_journald_errors() {
        let source = MockLogSource::with_journald_sample();
        let tool = AnalyzeErrors;
        let result = tool
            .execute(json!({"path": "/var/log/journal.export"}), &source)
            .await
            .unwrap();
        assert!(result.success);
        let data = result.data.as_ref().unwrap();
        assert!(data["total_lines"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn classify_connection_errors() {
        let mut source = MockLogSource::new();
        source.add_file(
            "/test.log",
            vec![
                r#"{"level":"error","message":"connection refused to database"}"#.into(),
                r#"{"level":"error","message":"ECONNRESET on socket"}"#.into(),
                r#"{"level":"error","message":"connection timed out"}"#.into(),
            ],
        );
        let tool = AnalyzeErrors;
        let result = tool
            .execute(json!({"path": "/test.log"}), &source)
            .await
            .unwrap();
        let data = result.data.as_ref().unwrap();
        let patterns = data["patterns"].as_array().unwrap();
        let conn = patterns
            .iter()
            .find(|p| p["category"] == "connection_error");
        assert!(conn.is_some(), "should classify as connection_error");
        assert_eq!(conn.unwrap()["count"].as_u64().unwrap(), 3);
    }

    #[tokio::test]
    async fn classify_permission_errors() {
        let mut source = MockLogSource::new();
        source.add_file(
            "/test.log",
            vec![
                r#"{"level":"error","message":"permission denied opening /dev/can0"}"#.into(),
                r#"{"level":"error","message":"access denied for user admin"}"#.into(),
            ],
        );
        let tool = AnalyzeErrors;
        let result = tool
            .execute(json!({"path": "/test.log"}), &source)
            .await
            .unwrap();
        let data = result.data.as_ref().unwrap();
        let patterns = data["patterns"].as_array().unwrap();
        let perm = patterns
            .iter()
            .find(|p| p["category"] == "permission_error");
        assert!(perm.is_some());
        assert_eq!(perm.unwrap()["count"].as_u64().unwrap(), 2);
    }

    #[tokio::test]
    async fn classify_can_bus_errors() {
        let mut source = MockLogSource::new();
        source.add_file(
            "/test.log",
            vec![
                r#"{"level":"error","message":"CAN bus timeout on can0"}"#.into(),
                r#"{"level":"error","message":"CAN interface error: device offline"}"#.into(),
            ],
        );
        let tool = AnalyzeErrors;
        let result = tool
            .execute(json!({"path": "/test.log"}), &source)
            .await
            .unwrap();
        let data = result.data.as_ref().unwrap();
        let patterns = data["patterns"].as_array().unwrap();
        let can = patterns.iter().find(|p| p["category"] == "can_bus_error");
        assert!(can.is_some());
    }

    #[tokio::test]
    async fn no_errors_returns_empty() {
        let mut source = MockLogSource::new();
        source.add_file(
            "/test.log",
            vec![
                r#"{"level":"info","message":"all good"}"#.into(),
                r#"{"level":"debug","message":"trace"}"#.into(),
            ],
        );
        let tool = AnalyzeErrors;
        let result = tool
            .execute(json!({"path": "/test.log"}), &source)
            .await
            .unwrap();
        let data = result.data.as_ref().unwrap();
        assert_eq!(data["error_count"].as_u64().unwrap(), 0);
        assert_eq!(data["classification_rate"].as_f64().unwrap(), 100.0);
    }
}
