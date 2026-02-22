//! Newline-delimited JSON (NDJSON) log parser.

use chrono::{DateTime, Utc};
use std::collections::HashMap;

use crate::types::{LogEntry, LogFormat, LogSeverity};

/// Parse a single JSON line into a LogEntry.
pub fn parse_line(line: &str, line_number: usize) -> Option<LogEntry> {
    let obj: serde_json::Value = serde_json::from_str(line).ok()?;
    let map = obj.as_object()?;

    // Extract message — try common field names
    let message = extract_string(map, &["message", "msg", "text", "log"])?;

    // Extract severity
    let severity = extract_string(map, &["level", "severity", "loglevel", "priority"])
        .map(|s| parse_severity(&s))
        .unwrap_or(LogSeverity::Info);

    // Extract timestamp
    let timestamp = extract_string(map, &["timestamp", "time", "ts", "@timestamp", "datetime"])
        .and_then(|s| parse_timestamp(&s));

    // Extract source
    let source = extract_string(
        map,
        &["service", "source", "app", "application", "logger", "name"],
    );

    // Collect remaining fields as extras
    let skip = [
        "message",
        "msg",
        "text",
        "log",
        "level",
        "severity",
        "loglevel",
        "priority",
        "timestamp",
        "time",
        "ts",
        "@timestamp",
        "datetime",
        "service",
        "source",
        "app",
        "application",
        "logger",
        "name",
    ];
    let fields: HashMap<String, String> = map
        .iter()
        .filter(|(k, _)| !skip.contains(&k.as_str()))
        .map(|(k, v)| {
            let val = match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            (k.clone(), val)
        })
        .collect();

    Some(LogEntry {
        timestamp,
        severity,
        source,
        message,
        raw: line.to_string(),
        line_number,
        format: LogFormat::JsonLines,
        fields,
    })
}

/// Check if a line looks like JSON.
pub fn looks_like_json(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('{') && trimmed.ends_with('}')
}

fn extract_string(
    map: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    for key in keys {
        if let Some(val) = map.get(*key) {
            return match val {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                other => Some(other.to_string()),
            };
        }
    }
    None
}

fn parse_severity(s: &str) -> LogSeverity {
    match s.to_lowercase().as_str() {
        "debug" | "trace" | "7" => LogSeverity::Debug,
        "info" | "informational" | "6" => LogSeverity::Info,
        "notice" | "5" => LogSeverity::Notice,
        "warn" | "warning" | "4" => LogSeverity::Warning,
        "error" | "err" | "3" => LogSeverity::Error,
        "critical" | "crit" | "fatal" | "alert" | "emergency" | "emerg" | "panic" | "0" | "1"
        | "2" => LogSeverity::Critical,
        _ => LogSeverity::Info,
    }
}

fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    // Try RFC 3339 first
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            // Try without timezone (assume UTC)
            chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|ndt| ndt.and_utc())
        })
        .or_else(|| {
            // Try epoch seconds
            s.parse::<f64>().ok().and_then(|secs| {
                DateTime::from_timestamp(secs as i64, ((secs.fract()) * 1e9) as u32)
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_standard_json_log() {
        let line = r#"{"timestamp":"2024-01-15T12:00:05Z","level":"error","message":"CAN bus timeout","service":"canbus"}"#;
        let entry = parse_line(line, 1).unwrap();
        assert_eq!(entry.severity, LogSeverity::Error);
        assert_eq!(entry.message, "CAN bus timeout");
        assert_eq!(entry.source.as_deref(), Some("canbus"));
        assert!(entry.timestamp.is_some());
        assert_eq!(entry.format, LogFormat::JsonLines);
    }

    #[test]
    fn parse_alternative_field_names() {
        let line = r#"{"time":"2024-01-15T12:00:00Z","severity":"warn","msg":"Low battery","app":"monitor"}"#;
        let entry = parse_line(line, 1).unwrap();
        assert_eq!(entry.severity, LogSeverity::Warning);
        assert_eq!(entry.message, "Low battery");
        assert_eq!(entry.source.as_deref(), Some("monitor"));
    }

    #[test]
    fn parse_extra_fields_preserved() {
        let line = r#"{"timestamp":"2024-01-15T12:00:00Z","level":"info","message":"test","error_code":"E001","retry":3}"#;
        let entry = parse_line(line, 1).unwrap();
        assert_eq!(
            entry.fields.get("error_code").map(|s| s.as_str()),
            Some("E001")
        );
        assert_eq!(entry.fields.get("retry").map(|s| s.as_str()), Some("3"));
    }

    #[test]
    fn parse_fatal_as_critical() {
        let line = r#"{"level":"fatal","message":"unrecoverable error"}"#;
        let entry = parse_line(line, 1).unwrap();
        assert_eq!(entry.severity, LogSeverity::Critical);
    }

    #[test]
    fn parse_numeric_severity() {
        let line = r#"{"priority":"3","message":"error level"}"#;
        let entry = parse_line(line, 1).unwrap();
        assert_eq!(entry.severity, LogSeverity::Error);
    }

    #[test]
    fn invalid_json_returns_none() {
        assert!(parse_line("not json", 1).is_none());
        assert!(parse_line("{incomplete", 1).is_none());
    }

    #[test]
    fn json_without_message_returns_none() {
        assert!(parse_line(r#"{"level":"info","no_msg_field":true}"#, 1).is_none());
    }

    #[test]
    fn detect_json_format() {
        assert!(looks_like_json(r#"{"key":"value"}"#));
        assert!(looks_like_json(r#"  {"key":"value"}  "#));
        assert!(!looks_like_json("plain text"));
        assert!(!looks_like_json("<134>syslog line"));
    }
}
