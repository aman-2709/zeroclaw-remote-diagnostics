//! Plaintext (unstructured) log parser with heuristic severity detection.

use chrono::{DateTime, NaiveDateTime, Utc};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

use crate::types::{LogEntry, LogFormat, LogSeverity};

// Common timestamp patterns
static RE_ISO_TS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?)")
        .unwrap()
});

static RE_COMMON_TS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d{4}-\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2})").unwrap());

// Severity detection patterns — checked in order from most to least severe
static SEVERITY_PATTERNS: LazyLock<Vec<(Regex, LogSeverity)>> = LazyLock::new(|| {
    vec![
        (
            Regex::new(r"(?i)\b(CRITICAL|CRIT|FATAL|EMERG|EMERGENCY|ALERT|PANIC)\b").unwrap(),
            LogSeverity::Critical,
        ),
        (
            Regex::new(r"(?i)\b(ERROR|ERR|FAIL(?:ED|URE)?)\b").unwrap(),
            LogSeverity::Error,
        ),
        (
            Regex::new(r"(?i)\b(WARN(?:ING)?)\b").unwrap(),
            LogSeverity::Warning,
        ),
        (
            Regex::new(r"(?i)\b(NOTICE)\b").unwrap(),
            LogSeverity::Notice,
        ),
        (Regex::new(r"(?i)\b(INFO)\b").unwrap(), LogSeverity::Info),
        (
            Regex::new(r"(?i)\b(DEBUG|TRACE)\b").unwrap(),
            LogSeverity::Debug,
        ),
    ]
});

/// Parse an unstructured plaintext log line using heuristics.
pub fn parse_line(line: &str, line_number: usize) -> LogEntry {
    let severity = detect_severity(line);
    let timestamp = detect_timestamp(line);

    LogEntry {
        timestamp,
        severity,
        source: None,
        message: line.to_string(),
        raw: line.to_string(),
        line_number,
        format: LogFormat::Plaintext,
        fields: HashMap::new(),
    }
}

/// Detect severity from keywords in the line.
pub fn detect_severity(line: &str) -> LogSeverity {
    for (pattern, severity) in SEVERITY_PATTERNS.iter() {
        if pattern.is_match(line) {
            return *severity;
        }
    }
    LogSeverity::Info // default for lines without severity indicators
}

/// Try to extract a timestamp from a plaintext log line.
fn detect_timestamp(line: &str) -> Option<DateTime<Utc>> {
    // Try ISO 8601 / RFC 3339
    if let Some(caps) = RE_ISO_TS.captures(line) {
        if let Ok(dt) = DateTime::parse_from_rfc3339(&caps[1]) {
            return Some(dt.with_timezone(&Utc));
        }
        if let Ok(ndt) = NaiveDateTime::parse_from_str(&caps[1], "%Y-%m-%dT%H:%M:%S%.f") {
            return Some(ndt.and_utc());
        }
    }

    // Try common format: "2024-01-15 12:34:56"
    if let Some(caps) = RE_COMMON_TS.captures(line)
        && let Ok(ndt) = NaiveDateTime::parse_from_str(&caps[1], "%Y-%m-%d %H:%M:%S")
    {
        return Some(ndt.and_utc());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_error_severity() {
        assert_eq!(
            detect_severity("2024-01-15 ERROR: something broke"),
            LogSeverity::Error
        );
        assert_eq!(
            detect_severity("CRITICAL: disk full"),
            LogSeverity::Critical
        );
        assert_eq!(detect_severity("Warning: low memory"), LogSeverity::Warning);
        assert_eq!(detect_severity("INFO: all good"), LogSeverity::Info);
        assert_eq!(detect_severity("DEBUG: trace output"), LogSeverity::Debug);
    }

    #[test]
    fn detect_failed_as_error() {
        assert_eq!(detect_severity("Connection FAILED"), LogSeverity::Error);
        assert_eq!(detect_severity("Task failure detected"), LogSeverity::Error);
    }

    #[test]
    fn detect_fatal_as_critical() {
        assert_eq!(
            detect_severity("FATAL: cannot continue"),
            LogSeverity::Critical
        );
        assert_eq!(
            detect_severity("PANIC: kernel error"),
            LogSeverity::Critical
        );
    }

    #[test]
    fn default_severity_is_info() {
        assert_eq!(
            detect_severity("Just a plain log message"),
            LogSeverity::Info
        );
    }

    #[test]
    fn detect_iso_timestamp() {
        let entry = parse_line("2024-01-15T12:34:56Z ERROR: test", 1);
        assert!(entry.timestamp.is_some());
        assert_eq!(entry.severity, LogSeverity::Error);
    }

    #[test]
    fn detect_common_timestamp() {
        let entry = parse_line("2024-01-15 12:34:56 INFO Starting up", 1);
        assert!(entry.timestamp.is_some());
        assert_eq!(entry.severity, LogSeverity::Info);
    }

    #[test]
    fn no_timestamp_is_fine() {
        let entry = parse_line("Just an error message ERROR here", 1);
        assert!(entry.timestamp.is_none());
        assert_eq!(entry.severity, LogSeverity::Error);
    }

    #[test]
    fn plaintext_format_set() {
        let entry = parse_line("anything", 1);
        assert_eq!(entry.format, LogFormat::Plaintext);
    }
}
