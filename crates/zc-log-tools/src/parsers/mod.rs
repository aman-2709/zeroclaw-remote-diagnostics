//! Multi-format log parsers with auto-detection.
//!
//! Supports syslog (RFC 3164/5424), systemd journald export, newline-delimited
//! JSON, and plaintext with heuristic severity detection.

pub mod journald;
pub mod json_lines;
pub mod plaintext;
pub mod syslog;

use crate::types::{LogEntry, LogFormat};

/// Parse a single line using the specified format.
///
/// Returns `None` if the line cannot be parsed in the given format.
/// For journald (which is multi-line), use `parse_lines` instead.
pub fn parse_line(line: &str, line_number: usize, format: LogFormat) -> Option<LogEntry> {
    match format {
        LogFormat::Syslog3164 => syslog::parse_3164(line, line_number),
        LogFormat::Syslog5424 => syslog::parse_5424(line, line_number),
        LogFormat::JsonLines => json_lines::parse_line(line, line_number),
        LogFormat::Plaintext => Some(plaintext::parse_line(line, line_number)),
        // Journald uses multi-line parsing — single-line parse not applicable
        LogFormat::Journald => None,
    }
}

/// Parse all lines using the specified format.
///
/// Handles multi-line formats (journald) and single-line formats alike.
pub fn parse_lines(lines: &[String], format: LogFormat) -> Vec<LogEntry> {
    if format == LogFormat::Journald {
        return journald::parse_entries(lines);
    }
    lines
        .iter()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .filter_map(|(i, line)| parse_line(line, i + 1, format))
        .collect()
}

/// Auto-detect the log format from a sample of lines.
pub fn detect_format(lines: &[String]) -> LogFormat {
    if lines.is_empty() {
        return LogFormat::Plaintext;
    }

    // Check for journald (multi-line key=value with known keys)
    if journald::looks_like_journald(lines) {
        return LogFormat::Journald;
    }

    // Sample first non-empty lines for single-line format detection
    let sample: Vec<&str> = lines
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !s.trim().is_empty())
        .take(5)
        .collect();

    if sample.is_empty() {
        return LogFormat::Plaintext;
    }

    let mut json_count = 0;
    let mut syslog_count = 0;

    for line in &sample {
        if json_lines::looks_like_json(line) {
            json_count += 1;
        } else if syslog::looks_like_syslog(line) {
            syslog_count += 1;
        }
    }

    // Majority vote
    if json_count > sample.len() / 2 {
        return LogFormat::JsonLines;
    }

    if syslog_count > sample.len() / 2 {
        // Distinguish 3164 vs 5424 from first syslog line
        for line in &sample {
            if syslog::looks_like_syslog(line) {
                if syslog::parse_5424(line, 0).is_some() {
                    return LogFormat::Syslog5424;
                }
                return LogFormat::Syslog3164;
            }
        }
        return LogFormat::Syslog3164;
    }

    LogFormat::Plaintext
}

/// Parse lines with auto-format detection.
pub fn auto_parse(lines: &[String]) -> Vec<LogEntry> {
    let format = detect_format(lines);
    parse_lines(lines, format)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_syslog_3164() {
        let lines = vec![
            "<134>Jan 15 12:00:01 edge1 myapp: test".into(),
            "<131>Jan 15 12:00:05 edge1 myapp: error".into(),
        ];
        assert_eq!(detect_format(&lines), LogFormat::Syslog3164);
    }

    #[test]
    fn detect_syslog_5424() {
        let lines = vec![
            "<134>1 2024-01-15T12:00:00Z edge1 app 1 - - test".into(),
            "<131>1 2024-01-15T12:00:05Z edge1 app 1 - - error".into(),
        ];
        assert_eq!(detect_format(&lines), LogFormat::Syslog5424);
    }

    #[test]
    fn detect_json_format() {
        let lines = vec![
            r#"{"level":"info","message":"test"}"#.into(),
            r#"{"level":"error","message":"fail"}"#.into(),
        ];
        assert_eq!(detect_format(&lines), LogFormat::JsonLines);
    }

    #[test]
    fn detect_journald_format() {
        let lines = vec![
            "__REALTIME_TIMESTAMP=1234567890".into(),
            "PRIORITY=6".into(),
            "MESSAGE=test".into(),
        ];
        assert_eq!(detect_format(&lines), LogFormat::Journald);
    }

    #[test]
    fn detect_plaintext_format() {
        let lines = vec!["Just a plain log line".into(), "Another plain line".into()];
        assert_eq!(detect_format(&lines), LogFormat::Plaintext);
    }

    #[test]
    fn detect_empty_is_plaintext() {
        assert_eq!(detect_format(&[]), LogFormat::Plaintext);
    }

    #[test]
    fn auto_parse_syslog() {
        let lines = vec![
            "<134>Jan 15 12:00:01 edge1 myapp[1234]: Starting".into(),
            "<131>Jan 15 12:00:05 edge1 myapp[1234]: Error occurred".into(),
        ];
        let entries = auto_parse(&lines);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].severity, crate::types::LogSeverity::Info);
        assert_eq!(entries[1].severity, crate::types::LogSeverity::Error);
    }

    #[test]
    fn auto_parse_json() {
        let lines = vec![
            r#"{"level":"error","message":"fail"}"#.into(),
            r#"{"level":"info","message":"ok"}"#.into(),
        ];
        let entries = auto_parse(&lines);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].severity, crate::types::LogSeverity::Error);
    }

    #[test]
    fn auto_parse_journald() {
        let lines = vec![
            "__REALTIME_TIMESTAMP=1705312801000000".into(),
            "PRIORITY=3".into(),
            "MESSAGE=error msg".into(),
            "".into(),
        ];
        let entries = auto_parse(&lines);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].severity, crate::types::LogSeverity::Error);
    }

    #[test]
    fn parse_lines_skips_empty() {
        let lines = vec![
            r#"{"level":"info","message":"a"}"#.into(),
            "".into(),
            r#"{"level":"info","message":"b"}"#.into(),
        ];
        let entries = parse_lines(&lines, LogFormat::JsonLines);
        assert_eq!(entries.len(), 2);
    }
}
