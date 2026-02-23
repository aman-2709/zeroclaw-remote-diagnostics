//! query_journal — Query systemd journal for a service unit via journalctl subprocess.
//!
//! Unlike the other 4 log tools, this bypasses `LogSource` and runs `journalctl`
//! directly as a child process. Output is parsed via the existing journald export
//! parser.

use async_trait::async_trait;
use serde_json::json;
use std::time::Duration;
use tokio::process::Command;

use crate::error::LogResult;
use crate::parsers::journald;
use crate::source::LogSource;
use crate::types::{LogTool, ToolResult};

/// Maximum output size from journalctl (64 KB).
const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// Subprocess timeout.
const TIMEOUT: Duration = Duration::from_secs(5);

pub struct QueryJournal;

/// Validate a systemd unit name: only alphanumeric, `.`, `@`, `-`, `_`.
fn is_valid_unit_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '@' | '-' | '_'))
}

#[async_trait]
impl LogTool for QueryJournal {
    fn name(&self) -> &str {
        "query_journal"
    }

    fn description(&self) -> &str {
        "Query systemd journal for a service unit via journalctl"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "unit": {
                    "type": "string",
                    "description": "Systemd unit name (e.g. nginx.service)"
                },
                "lines": {
                    "type": "integer",
                    "description": "Number of recent journal entries (default: 50)",
                    "default": 50
                },
                "priority": {
                    "type": "string",
                    "enum": ["emerg", "alert", "crit", "err", "warning", "notice", "info", "debug"],
                    "description": "Maximum syslog priority level to include"
                },
                "since": {
                    "type": "string",
                    "description": "Show entries since this time (e.g. '1 hour ago', '2024-01-15')"
                }
            },
            "required": ["unit"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _source: &dyn LogSource,
    ) -> LogResult<ToolResult> {
        let unit = match args["unit"].as_str() {
            Some(u) if !u.is_empty() => u,
            _ => {
                return Ok(ToolResult::failure(
                    "query_journal",
                    "missing required 'unit' argument",
                ));
            }
        };

        if !is_valid_unit_name(unit) {
            return Ok(ToolResult::failure(
                "query_journal",
                format!("invalid unit name: {unit}"),
            ));
        }

        let lines = args["lines"].as_u64().unwrap_or(50);
        let priority = args["priority"].as_str();
        let since = args["since"].as_str();

        let mut cmd = Command::new("journalctl");
        cmd.arg("--output=export")
            .arg("--no-pager")
            .arg(format!("--lines={lines}"))
            .arg(format!("--unit={unit}"));

        if let Some(p) = priority {
            cmd.arg(format!("--priority={p}"));
        }
        if let Some(s) = since {
            // Validate since: same charset as unit + spaces and colons for timestamps
            if s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | ':' | '.' | '/'))
            {
                cmd.arg(format!("--since={s}"));
            }
        }

        let result = match tokio::time::timeout(TIMEOUT, cmd.output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                return Ok(ToolResult::failure(
                    "query_journal",
                    format!("failed to run journalctl: {e}"),
                ));
            }
            Err(_) => {
                return Ok(ToolResult::failure(
                    "query_journal",
                    "journalctl timed out after 5s",
                ));
            }
        };

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            return Ok(ToolResult::failure(
                "query_journal",
                format!(
                    "journalctl exited with {}: {}",
                    result.status,
                    stderr.trim()
                ),
            ));
        }

        // Cap output size
        let stdout = if result.stdout.len() > MAX_OUTPUT_BYTES {
            &result.stdout[..MAX_OUTPUT_BYTES]
        } else {
            &result.stdout
        };

        let raw = String::from_utf8_lossy(stdout);
        let output_lines: Vec<String> = raw.lines().map(|l| l.to_string()).collect();
        let entries = journald::parse_entries(&output_lines);

        let entry_json: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                json!({
                    "severity": e.severity.as_str(),
                    "message": e.message,
                    "timestamp": e.timestamp,
                    "source": e.source,
                })
            })
            .collect();

        let count = entry_json.len();
        let data = json!({
            "unit": unit,
            "entries": entry_json,
            "entry_count": count,
        });

        Ok(ToolResult::success(
            "query_journal",
            data,
            format!("Retrieved {count} journal entries for {unit}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockLogSource;

    #[test]
    fn schema_has_required_unit() {
        let tool = QueryJournal;
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["unit"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("unit")));
    }

    #[test]
    fn valid_unit_names() {
        assert!(is_valid_unit_name("nginx.service"));
        assert!(is_valid_unit_name("systemd-journald.service"));
        assert!(is_valid_unit_name("user@1000.service"));
        assert!(is_valid_unit_name("my_app"));
    }

    #[test]
    fn invalid_unit_names() {
        assert!(!is_valid_unit_name(""));
        assert!(!is_valid_unit_name("foo;bar"));
        assert!(!is_valid_unit_name("$(evil)"));
        assert!(!is_valid_unit_name("unit name"));
        assert!(!is_valid_unit_name("a|b"));
    }

    #[tokio::test]
    async fn missing_unit_arg_returns_failure() {
        let tool = QueryJournal;
        let source = MockLogSource::new();
        let result = tool.execute(json!({}), &source).await.unwrap();
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("missing"));
    }

    #[tokio::test]
    async fn invalid_unit_returns_failure() {
        let tool = QueryJournal;
        let source = MockLogSource::new();
        let result = tool
            .execute(json!({"unit": "$(whoami)"}), &source)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("invalid unit name"));
    }

    /// Integration test: runs real journalctl. Only passes on Linux with systemd.
    #[tokio::test]
    #[ignore] // Requires systemd — run with `cargo test -- --ignored`
    async fn live_journalctl_query() {
        let tool = QueryJournal;
        let source = MockLogSource::new();
        let result = tool
            .execute(
                json!({"unit": "systemd-journald.service", "lines": 5}),
                &source,
            )
            .await
            .unwrap();
        assert!(result.success, "journalctl should succeed: {result:?}");
        let count = result.data.as_ref().unwrap()["entry_count"]
            .as_u64()
            .unwrap();
        assert!(count <= 5);
    }
}
