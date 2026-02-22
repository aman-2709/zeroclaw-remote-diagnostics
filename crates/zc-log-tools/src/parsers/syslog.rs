//! Syslog parsers for RFC 3164 (BSD) and RFC 5424 (IETF).

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

use crate::types::{LogEntry, LogFormat, LogSeverity};

// RFC 3164: <PRI>Mmm dd HH:MM:SS HOSTNAME TAG[PID]: MSG
static RE_3164: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^<(\d{1,3})>(\w{3}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2})\s+(\S+)\s+(.+)$").unwrap()
});

// RFC 5424: <PRI>VER TIMESTAMP HOSTNAME APP PROCID MSGID [SD] MSG
static RE_5424: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^<(\d{1,3})>(\d+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s*(.*)$").unwrap()
});

/// Try to parse a line as RFC 3164 syslog.
pub fn parse_3164(line: &str, line_number: usize) -> Option<LogEntry> {
    let caps = RE_3164.captures(line)?;
    let pri: u8 = caps[1].parse().ok()?;
    let severity = LogSeverity::from_syslog_severity(pri & 0x07);
    let timestamp_str = &caps[2];
    let hostname = &caps[3];
    let remainder = &caps[4];

    let ts = parse_bsd_timestamp(timestamp_str);
    let (tag, message) = split_tag_message(remainder);

    let source = if tag.is_empty() {
        Some(hostname.to_string())
    } else {
        Some(format!("{hostname}/{tag}"))
    };

    Some(LogEntry {
        timestamp: ts,
        severity,
        source,
        message: message.to_string(),
        raw: line.to_string(),
        line_number,
        format: LogFormat::Syslog3164,
        fields: HashMap::new(),
    })
}

/// Try to parse a line as RFC 5424 syslog.
pub fn parse_5424(line: &str, line_number: usize) -> Option<LogEntry> {
    let caps = RE_5424.captures(line)?;
    let pri: u8 = caps[1].parse().ok()?;
    let severity = LogSeverity::from_syslog_severity(pri & 0x07);
    let timestamp_str = &caps[3];
    let hostname = &caps[4];
    let app_name = &caps[5];
    let msg_part = &caps[8];

    let ts = DateTime::parse_from_rfc3339(timestamp_str)
        .ok()
        .map(|dt| dt.with_timezone(&Utc));

    // Strip structured data [SD] prefix if present
    let message = strip_structured_data(msg_part);

    let source = if app_name == "-" {
        Some(hostname.to_string())
    } else {
        Some(format!("{hostname}/{app_name}"))
    };

    Some(LogEntry {
        timestamp: ts,
        severity,
        source,
        message: message.to_string(),
        raw: line.to_string(),
        line_number,
        format: LogFormat::Syslog5424,
        fields: HashMap::new(),
    })
}

/// Try to parse a line as either RFC 3164 or 5424.
pub fn parse(line: &str, line_number: usize) -> Option<LogEntry> {
    // Try 5424 first (more specific — has a version digit after PRI)
    parse_5424(line, line_number).or_else(|| parse_3164(line, line_number))
}

/// Check if a line looks like syslog (starts with `<PRI>`).
pub fn looks_like_syslog(line: &str) -> bool {
    line.starts_with('<')
        && line.len() > 3
        && line[1..].chars().take_while(|c| c.is_ascii_digit()).count() > 0
}

fn parse_bsd_timestamp(s: &str) -> Option<DateTime<Utc>> {
    // Format: "Jan 15 12:34:56" or "Jan  5 12:34:56"
    let year = Utc::now().format("%Y");
    let with_year = format!("{year} {s}");
    NaiveDateTime::parse_from_str(&with_year, "%Y %b %e %H:%M:%S")
        .ok()
        .map(|ndt| Utc.from_utc_datetime(&ndt))
}

fn split_tag_message(s: &str) -> (&str, &str) {
    // TAG[PID]: MSG  or  TAG: MSG  or  just MSG
    if let Some(colon_pos) = s.find(": ") {
        let tag_part = &s[..colon_pos];
        let msg = &s[colon_pos + 2..];
        // Strip [PID] from tag
        let tag = tag_part.split('[').next().unwrap_or(tag_part);
        (tag, msg)
    } else {
        ("", s)
    }
}

fn strip_structured_data(s: &str) -> &str {
    let s = s.trim();
    // NILVALUE: SD is "-" meaning no structured data
    if s == "-" {
        return "";
    }
    if s.starts_with("- ") {
        return &s[2..];
    }
    if s.starts_with('[') {
        let mut depth = 0;
        for (i, c) in s.char_indices() {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        return s[i + 1..].trim_start();
                    }
                }
                _ => {}
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rfc3164_basic() {
        let line = "<134>Jan 15 12:00:01 edge1 myapp[1234]: Service started successfully";
        let entry = parse_3164(line, 1).unwrap();
        assert_eq!(entry.severity, LogSeverity::Info); // 134 & 7 = 6
        assert_eq!(entry.source.as_deref(), Some("edge1/myapp"));
        assert_eq!(entry.message, "Service started successfully");
        assert_eq!(entry.format, LogFormat::Syslog3164);
        assert!(entry.timestamp.is_some());
    }

    #[test]
    fn parse_rfc3164_error() {
        let line = "<131>Jan 15 12:00:05 edge1 myapp[1234]: Connection refused";
        let entry = parse_3164(line, 2).unwrap();
        assert_eq!(entry.severity, LogSeverity::Error); // 131 & 7 = 3
    }

    #[test]
    fn parse_rfc3164_critical() {
        let line = "<128>Jan 15 12:00:15 edge1 kernel: System halted";
        let entry = parse_3164(line, 3).unwrap();
        assert_eq!(entry.severity, LogSeverity::Critical); // 128 & 7 = 0
    }

    #[test]
    fn parse_rfc3164_no_pid() {
        let line = "<134>Jan 15 12:00:10 edge1 kernel: [12345.678] eth0: link up";
        let entry = parse_3164(line, 1).unwrap();
        assert_eq!(entry.source.as_deref(), Some("edge1/kernel"));
    }

    #[test]
    fn parse_rfc5424_basic() {
        let line = "<165>1 2024-01-15T12:34:56.789Z myhost myapp 1234 ID47 [exampleSDID@32473 iut=\"3\"] An application event";
        let entry = parse_5424(line, 1).unwrap();
        assert_eq!(entry.severity, LogSeverity::Notice); // 165 & 7 = 5
        assert_eq!(entry.source.as_deref(), Some("myhost/myapp"));
        assert_eq!(entry.message, "An application event");
        assert_eq!(entry.format, LogFormat::Syslog5424);
        assert!(entry.timestamp.is_some());
    }

    #[test]
    fn parse_rfc5424_no_sd() {
        let line = "<134>1 2024-01-15T12:00:00Z edge1 agent 999 - - Service ready";
        let entry = parse_5424(line, 1).unwrap();
        assert_eq!(entry.severity, LogSeverity::Info);
        assert_eq!(entry.message, "Service ready");
    }

    #[test]
    fn parse_auto_selects_format() {
        let rfc3164 = "<134>Jan 15 12:00:01 edge1 myapp[1234]: Hello";
        let entry = parse(rfc3164, 1).unwrap();
        assert_eq!(entry.format, LogFormat::Syslog3164);

        let rfc5424 = "<134>1 2024-01-15T12:00:00Z edge1 app 1 - - Hello";
        let entry = parse(rfc5424, 1).unwrap();
        assert_eq!(entry.format, LogFormat::Syslog5424);
    }

    #[test]
    fn looks_like_syslog_positive() {
        assert!(looks_like_syslog("<134>Jan 15 12:00:01 host msg"));
        assert!(looks_like_syslog("<0>test"));
    }

    #[test]
    fn looks_like_syslog_negative() {
        assert!(!looks_like_syslog("plain text"));
        assert!(!looks_like_syslog("{\"json\":true}"));
        assert!(!looks_like_syslog("<>empty pri"));
    }
}
