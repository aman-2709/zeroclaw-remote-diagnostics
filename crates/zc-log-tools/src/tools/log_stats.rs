//! log_stats — compute log statistics: severity counts, time range, top sources.

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;

use crate::error::{LogError, LogResult};
use crate::parsers;
use crate::source::LogSource;
use crate::types::{LogFormat, LogSeverity, LogTool, ToolResult};

pub struct LogStats;

#[async_trait]
impl LogTool for LogStats {
    fn name(&self) -> &str {
        "log_stats"
    }

    fn description(&self) -> &str {
        "Compute log statistics: severity counts, time range, top sources"
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

        // Severity counts
        let mut severity_counts: HashMap<LogSeverity, usize> = HashMap::new();
        for entry in &entries {
            *severity_counts.entry(entry.severity).or_default() += 1;
        }

        // Source counts (top 10)
        let mut source_counts: HashMap<String, usize> = HashMap::new();
        for entry in &entries {
            if let Some(ref src) = entry.source {
                *source_counts.entry(src.clone()).or_default() += 1;
            }
        }
        let mut top_sources: Vec<_> = source_counts.into_iter().collect();
        top_sources.sort_by(|a, b| b.1.cmp(&a.1));
        top_sources.truncate(10);

        // Time range
        let timestamps: Vec<_> = entries.iter().filter_map(|e| e.timestamp).collect();
        let earliest = timestamps.iter().min();
        let latest = timestamps.iter().max();

        let total = entries.len();
        let error_count = severity_counts
            .get(&LogSeverity::Error)
            .copied()
            .unwrap_or(0)
            + severity_counts
                .get(&LogSeverity::Critical)
                .copied()
                .unwrap_or(0);

        let data = json!({
            "path": path,
            "format": format!("{fmt:?}"),
            "total_lines": lines.len(),
            "parsed_entries": total,
            "severity_counts": {
                "critical": severity_counts.get(&LogSeverity::Critical).copied().unwrap_or(0),
                "error": severity_counts.get(&LogSeverity::Error).copied().unwrap_or(0),
                "warning": severity_counts.get(&LogSeverity::Warning).copied().unwrap_or(0),
                "notice": severity_counts.get(&LogSeverity::Notice).copied().unwrap_or(0),
                "info": severity_counts.get(&LogSeverity::Info).copied().unwrap_or(0),
                "debug": severity_counts.get(&LogSeverity::Debug).copied().unwrap_or(0),
            },
            "time_range": {
                "earliest": earliest,
                "latest": latest,
            },
            "top_sources": top_sources.iter().map(|(src, count)| json!({
                "source": src,
                "count": count,
            })).collect::<Vec<_>>(),
        });

        Ok(ToolResult::success(
            "log_stats",
            data,
            format!("{total} entries: {error_count} errors/critical, from {path}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockLogSource;

    #[tokio::test]
    async fn stats_syslog() {
        let source = MockLogSource::with_syslog_sample();
        let tool = LogStats;
        let result = tool
            .execute(json!({"path": "/var/log/syslog"}), &source)
            .await
            .unwrap();
        assert!(result.success);
        let data = result.data.as_ref().unwrap();
        assert!(data["parsed_entries"].as_u64().unwrap() > 0);
        assert!(data["severity_counts"]["error"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn stats_json() {
        let source = MockLogSource::with_json_sample();
        let tool = LogStats;
        let result = tool
            .execute(json!({"path": "/var/log/app.json"}), &source)
            .await
            .unwrap();
        assert!(result.success);
        let data = result.data.as_ref().unwrap();
        assert_eq!(data["parsed_entries"].as_u64().unwrap(), 8);
        assert!(!data["top_sources"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn stats_journald() {
        let source = MockLogSource::with_journald_sample();
        let tool = LogStats;
        let result = tool
            .execute(json!({"path": "/var/log/journal.export"}), &source)
            .await
            .unwrap();
        assert!(result.success);
        let data = result.data.as_ref().unwrap();
        assert_eq!(data["parsed_entries"].as_u64().unwrap(), 3);
    }

    #[tokio::test]
    async fn stats_empty_file() {
        let mut source = MockLogSource::new();
        source.add_file("/empty.log", vec![]);
        let tool = LogStats;
        let result = tool
            .execute(json!({"path": "/empty.log"}), &source)
            .await
            .unwrap();
        assert!(result.success);
        let data = result.data.as_ref().unwrap();
        assert_eq!(data["parsed_entries"].as_u64().unwrap(), 0);
    }

    #[tokio::test]
    async fn stats_has_time_range() {
        let source = MockLogSource::with_json_sample();
        let tool = LogStats;
        let result = tool
            .execute(json!({"path": "/var/log/app.json"}), &source)
            .await
            .unwrap();
        let data = result.data.as_ref().unwrap();
        assert!(data["time_range"]["earliest"].is_string());
        assert!(data["time_range"]["latest"].is_string());
    }

    #[tokio::test]
    async fn stats_top_sources_sorted() {
        let source = MockLogSource::with_json_sample();
        let tool = LogStats;
        let result = tool
            .execute(json!({"path": "/var/log/app.json"}), &source)
            .await
            .unwrap();
        let data = result.data.as_ref().unwrap();
        let sources = data["top_sources"].as_array().unwrap();
        // Verify descending order
        let counts: Vec<u64> = sources
            .iter()
            .map(|s| s["count"].as_u64().unwrap())
            .collect();
        for w in counts.windows(2) {
            assert!(w[0] >= w[1], "sources should be sorted by count descending");
        }
    }
}
