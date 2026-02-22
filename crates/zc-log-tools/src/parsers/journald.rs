//! systemd journal export format parser.
//!
//! Parses the text export format (`journalctl -o export`), where entries
//! are separated by blank lines and each field is `KEY=VALUE`.

use chrono::{TimeZone, Utc};
use std::collections::HashMap;

use crate::types::{LogEntry, LogFormat, LogSeverity};

/// Parse journald export format lines into log entries.
///
/// Entries are separated by blank lines. Each entry is a sequence of
/// `KEY=VALUE` lines.
pub fn parse_entries(lines: &[String]) -> Vec<LogEntry> {
    let mut entries = Vec::new();
    let mut current: HashMap<String, String> = HashMap::new();
    let mut entry_start_line = 1;

    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            if !current.is_empty() {
                if let Some(entry) = build_entry(&current, entry_start_line) {
                    entries.push(entry);
                }
                current.clear();
            }
            entry_start_line = i + 2; // next entry starts on the following line (1-based)
        } else if let Some((key, value)) = line.split_once('=') {
            if current.is_empty() {
                entry_start_line = i + 1;
            }
            current.insert(key.to_string(), value.to_string());
        }
    }

    // Handle last entry (no trailing blank line)
    if !current.is_empty()
        && let Some(entry) = build_entry(&current, entry_start_line)
    {
        entries.push(entry);
    }

    entries
}

fn build_entry(fields: &HashMap<String, String>, line_number: usize) -> Option<LogEntry> {
    let message = fields.get("MESSAGE")?.to_string();

    let timestamp = fields
        .get("__REALTIME_TIMESTAMP")
        .and_then(|ts| ts.parse::<i64>().ok())
        .and_then(|us| Utc.timestamp_micros(us).single());

    let severity = fields
        .get("PRIORITY")
        .and_then(|p| p.parse::<u8>().ok())
        .map(LogSeverity::from_syslog_severity)
        .unwrap_or(LogSeverity::Info);

    let hostname = fields.get("_HOSTNAME");
    let ident = fields.get("SYSLOG_IDENTIFIER");
    let source = match (hostname, ident) {
        (Some(h), Some(id)) => Some(format!("{h}/{id}")),
        (Some(h), None) => Some(h.clone()),
        (None, Some(id)) => Some(id.clone()),
        (None, None) => None,
    };

    // Collect extra fields (skip the ones we already extracted)
    let skip = [
        "MESSAGE",
        "__REALTIME_TIMESTAMP",
        "PRIORITY",
        "_HOSTNAME",
        "SYSLOG_IDENTIFIER",
    ];
    let extra: HashMap<String, String> = fields
        .iter()
        .filter(|(k, _)| !skip.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // Reconstruct a raw representation
    let raw = fields
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("\n");

    Some(LogEntry {
        timestamp,
        severity,
        source,
        message,
        raw,
        line_number,
        format: LogFormat::Journald,
        fields: extra,
    })
}

/// Check if lines look like journald export format.
pub fn looks_like_journald(lines: &[String]) -> bool {
    lines.iter().take(10).any(|l| {
        l.starts_with("__REALTIME_TIMESTAMP=")
            || l.starts_with("PRIORITY=")
            || l.starts_with("_HOSTNAME=")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_entry() {
        let lines = vec![
            "__REALTIME_TIMESTAMP=1705312801000000".into(),
            "_HOSTNAME=edge1".into(),
            "SYSLOG_IDENTIFIER=zeroclaw".into(),
            "PRIORITY=6".into(),
            "MESSAGE=Agent started".into(),
            "".into(),
        ];
        let entries = parse_entries(&lines);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].severity, LogSeverity::Info);
        assert_eq!(entries[0].message, "Agent started");
        assert_eq!(entries[0].source.as_deref(), Some("edge1/zeroclaw"));
        assert!(entries[0].timestamp.is_some());
    }

    #[test]
    fn parse_multiple_entries() {
        let lines = vec![
            "__REALTIME_TIMESTAMP=1705312801000000".into(),
            "PRIORITY=6".into(),
            "MESSAGE=First".into(),
            "".into(),
            "__REALTIME_TIMESTAMP=1705312802000000".into(),
            "PRIORITY=3".into(),
            "MESSAGE=Second".into(),
            "".into(),
        ];
        let entries = parse_entries(&lines);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].severity, LogSeverity::Info);
        assert_eq!(entries[1].severity, LogSeverity::Error);
    }

    #[test]
    fn parse_entry_without_trailing_blank() {
        let lines = vec!["PRIORITY=4".into(), "MESSAGE=Warning without blank".into()];
        let entries = parse_entries(&lines);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].severity, LogSeverity::Warning);
    }

    #[test]
    fn extra_fields_preserved() {
        let lines = vec![
            "PRIORITY=3".into(),
            "MESSAGE=error".into(),
            "_PID=1234".into(),
            "_SYSTEMD_UNIT=myapp.service".into(),
            "".into(),
        ];
        let entries = parse_entries(&lines);
        assert_eq!(entries[0].fields.get("_PID").unwrap(), "1234");
        assert_eq!(
            entries[0].fields.get("_SYSTEMD_UNIT").unwrap(),
            "myapp.service"
        );
    }

    #[test]
    fn detect_journald_format() {
        let yes = vec!["__REALTIME_TIMESTAMP=123456".into(), "MESSAGE=test".into()];
        assert!(looks_like_journald(&yes));

        let no = vec!["Just a plain line".into()];
        assert!(!looks_like_journald(&no));
    }

    #[test]
    fn skip_entry_without_message() {
        let lines = vec![
            "PRIORITY=6".into(),
            "_HOSTNAME=edge1".into(),
            // no MESSAGE field
            "".into(),
        ];
        let entries = parse_entries(&lines);
        assert_eq!(entries.len(), 0);
    }
}
