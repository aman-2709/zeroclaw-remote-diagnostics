//! Rule-based inference engine — pattern matching for known commands.
//!
//! Handles the common 80% of queries at zero cost and sub-millisecond latency.
//! Falls through to cloud inference (Bedrock) for anything it can't match.

use async_trait::async_trait;
use serde_json::json;

use super::{InferenceEngine, ParseResult};
use zc_protocol::commands::{ActionKind, ParsedIntent};

/// Pattern-matching inference engine for structured commands.
pub struct RuleBasedEngine;

impl RuleBasedEngine {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RuleBasedEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl InferenceEngine for RuleBasedEngine {
    async fn parse(&self, text: &str) -> Option<ParseResult> {
        parse_command(text).map(|intent| ParseResult {
            intent,
            tier: "local".into(),
        })
    }

    fn tier_name(&self) -> &str {
        "local"
    }
}

/// Core pattern matching logic.
fn parse_command(text: &str) -> Option<ParsedIntent> {
    let lower = text.to_lowercase();
    let lower = lower.trim();

    // ── CAN bus / OBD-II commands ───────────────────────────────

    // read_dtcs: "read dtcs", "get dtcs", "diagnostic trouble codes", "check engine codes"
    if matches_any(
        lower,
        &[
            "read dtc",
            "get dtc",
            "trouble code",
            "engine code",
            "check code",
            "fault code",
        ],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "read_dtcs".into(),
            tool_args: json!({}),
            confidence: 0.95,
        });
    }

    // read_vin: "read vin", "get vin", "vehicle identification"
    if matches_any(
        lower,
        &[
            "read vin",
            "get vin",
            "vehicle identification",
            "show vin",
            "what is the vin",
        ],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "read_vin".into(),
            tool_args: json!({}),
            confidence: 0.95,
        });
    }

    // read_freeze: "freeze frame", "freeze data", "snapshot data"
    if matches_any(
        lower,
        &[
            "freeze frame",
            "freeze data",
            "snapshot data",
            "read freeze",
        ],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "read_freeze".into(),
            tool_args: json!({}),
            confidence: 0.90,
        });
    }

    // read_pid: "read pid 0x0C", "read rpm", "read speed", "engine speed", etc.
    if let Some(intent) = try_parse_pid(lower) {
        return Some(intent);
    }

    // can_monitor: "monitor can", "sniff can", "capture can", "can bus traffic"
    if matches_any(
        lower,
        &[
            "monitor can",
            "sniff can",
            "capture can",
            "can bus traffic",
            "can traffic",
            "bus monitor",
        ],
    ) {
        let duration = extract_duration(lower).unwrap_or(10);
        return Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "can_monitor".into(),
            tool_args: json!({ "duration_secs": duration }),
            confidence: 0.90,
        });
    }

    // ── Log analysis commands ───────────────────────────────────

    // search_logs: "search logs for X", "grep logs", "find in logs"
    if matches_any(
        lower,
        &["search log", "grep log", "find in log", "search for"],
    ) {
        let query = extract_search_query(lower);
        return Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "search_logs".into(),
            tool_args: json!({
                "path": "/var/log/syslog",
                "query": query.unwrap_or("error"),
            }),
            confidence: if query.is_some() { 0.90 } else { 0.75 },
        });
    }

    // analyze_errors: "analyze errors", "error analysis", "what errors"
    if matches_any(
        lower,
        &[
            "analyze error",
            "error analysis",
            "what error",
            "find error",
            "show error",
        ],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "analyze_errors".into(),
            tool_args: json!({ "path": "/var/log/syslog" }),
            confidence: 0.90,
        });
    }

    // log_stats: "log stats", "log statistics", "log summary"
    if matches_any(
        lower,
        &["log stat", "log summar", "log overview", "show stat"],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "log_stats".into(),
            tool_args: json!({ "path": "/var/log/syslog" }),
            confidence: 0.90,
        });
    }

    // tail_logs: "tail logs", "recent logs", "latest logs", "show logs"
    if matches_any(
        lower,
        &[
            "tail log",
            "recent log",
            "latest log",
            "show log",
            "last log",
        ],
    ) {
        let lines = extract_line_count(lower).unwrap_or(50);
        return Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "tail_logs".into(),
            tool_args: json!({
                "path": "/var/log/syslog",
                "lines": lines,
            }),
            confidence: 0.85,
        });
    }

    // query_journal: "journal for X", "journalctl X", "service logs for X", "systemd logs"
    if matches_any(
        lower,
        &[
            "journal for",
            "journalctl",
            "service log",
            "systemd log",
            "show journal",
        ],
    ) {
        let unit = extract_service_name(lower).unwrap_or("systemd-journald.service");
        let lines = extract_line_count(lower).unwrap_or(50);
        return Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "query_journal".into(),
            tool_args: json!({
                "unit": unit,
                "lines": lines,
            }),
            confidence: if extract_service_name(lower).is_some() {
                0.90
            } else {
                0.75
            },
        });
    }

    None
}

/// Check if the text contains any of the given patterns.
fn matches_any(text: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| text.contains(p))
}

/// Try to parse a PID read command.
fn try_parse_pid(text: &str) -> Option<ParsedIntent> {
    // Named PIDs
    let named_pids = [
        (&["rpm", "engine speed", "engine rpm"][..], "0x0C"),
        (&["speed", "vehicle speed"][..], "0x0D"),
        (&["coolant", "coolant temp", "engine temp"][..], "0x05"),
        (&["throttle", "throttle position"][..], "0x11"),
        (&["fuel level", "fuel"][..], "0x2F"),
        (&["engine load", "load"][..], "0x04"),
        (&["intake temp", "intake air"][..], "0x0F"),
        (&["timing advance", "timing"][..], "0x0E"),
    ];

    for (keywords, pid) in &named_pids {
        if keywords.iter().any(|k| text.contains(k))
            && matches_any(text, &["read", "get", "show", "what", "check"])
        {
            return Some(ParsedIntent {
                action: ActionKind::Tool,
                tool_name: "read_pid".into(),
                tool_args: json!({ "pid": pid }),
                confidence: 0.92,
            });
        }
    }

    // Hex PID: "read pid 0x0C", "read pid 12"
    if text.contains("pid")
        && let Some(pid) = extract_hex_value(text)
    {
        return Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "read_pid".into(),
            tool_args: json!({ "pid": pid }),
            confidence: 0.95,
        });
    }

    None
}

/// Extract a hex PID value like "0x0C" or "0x2F" from text.
fn extract_hex_value(text: &str) -> Option<String> {
    for word in text.split_whitespace() {
        if word.starts_with("0x") || word.starts_with("0X") {
            return Some(word.to_uppercase());
        }
    }
    // Try decimal
    for word in text.split_whitespace() {
        if let Ok(n) = word.parse::<u8>() {
            return Some(format!("0x{n:02X}"));
        }
    }
    None
}

/// Extract duration in seconds from text like "for 30 seconds", "30s".
fn extract_duration(text: &str) -> Option<u32> {
    let words: Vec<&str> = text.split_whitespace().collect();
    for (i, word) in words.iter().enumerate() {
        // "30s" or "30sec"
        if let Some(stripped) = word.strip_suffix('s')
            && let Ok(n) = stripped.parse::<u32>()
        {
            return Some(n);
        }
        // "30 seconds" or "30 sec"
        if let Ok(n) = word.parse::<u32>()
            && i + 1 < words.len()
            && words[i + 1].starts_with("sec")
        {
            return Some(n);
        }
    }
    None
}

/// Extract a search query from "search logs for X" or "grep logs X".
fn extract_search_query(text: &str) -> Option<&str> {
    // "search logs for <query>"
    if let Some(pos) = text.find(" for ") {
        let query = text[pos + 5..].trim();
        if !query.is_empty() {
            return Some(query);
        }
    }
    // "grep logs <query>"
    if let Some(rest) = text.strip_prefix("grep log") {
        let query = rest.trim().trim_start_matches("s ");
        if !query.is_empty() {
            return Some(query);
        }
    }
    None
}

/// Extract a service/unit name from "journal for nginx", "journalctl sshd.service".
fn extract_service_name(text: &str) -> Option<&str> {
    // "journal for <service>"
    if let Some(pos) = text.find("journal for ") {
        let rest = text[pos + 12..].trim();
        let name = rest.split_whitespace().next()?;
        if !name.is_empty() {
            return Some(name);
        }
    }
    // "journalctl <service>"
    if let Some(rest) = text.strip_prefix("journalctl ") {
        let name = rest.split_whitespace().next()?;
        if !name.is_empty() && !name.starts_with('-') {
            return Some(name);
        }
    }
    // "service logs for <service>" / "service log for <service>"
    if let Some(pos) = text.find("service log") {
        let after = &text[pos..];
        if let Some(for_pos) = after.find(" for ") {
            let name = after[for_pos + 5..].split_whitespace().next()?;
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// Extract a line count from "last 100 logs", "tail 50".
fn extract_line_count(text: &str) -> Option<u32> {
    for word in text.split_whitespace() {
        if let Ok(n) = word.parse::<u32>()
            && n > 0
            && n <= 10000
        {
            return Some(n);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Option<ParsedIntent> {
        parse_command(text)
    }

    // ── DTC commands ────────────────────────────────────────────

    #[test]
    fn parse_read_dtcs() {
        let intent = parse("read DTCs").unwrap();
        assert_eq!(intent.tool_name, "read_dtcs");
        assert!(intent.confidence >= 0.9);
    }

    #[test]
    fn parse_get_trouble_codes() {
        let intent = parse("get diagnostic trouble codes").unwrap();
        assert_eq!(intent.tool_name, "read_dtcs");
    }

    #[test]
    fn parse_check_engine_codes() {
        let intent = parse("check engine codes").unwrap();
        assert_eq!(intent.tool_name, "read_dtcs");
    }

    // ── VIN commands ────────────────────────────────────────────

    #[test]
    fn parse_read_vin() {
        let intent = parse("read VIN").unwrap();
        assert_eq!(intent.tool_name, "read_vin");
    }

    #[test]
    fn parse_what_is_the_vin() {
        let intent = parse("what is the VIN?").unwrap();
        assert_eq!(intent.tool_name, "read_vin");
    }

    // ── Freeze frame ────────────────────────────────────────────

    #[test]
    fn parse_freeze_frame() {
        let intent = parse("read freeze frame data").unwrap();
        assert_eq!(intent.tool_name, "read_freeze");
    }

    // ── PID commands ────────────────────────────────────────────

    #[test]
    fn parse_read_rpm() {
        let intent = parse("read RPM").unwrap();
        assert_eq!(intent.tool_name, "read_pid");
        assert_eq!(intent.tool_args["pid"], "0x0C");
    }

    #[test]
    fn parse_get_vehicle_speed() {
        let intent = parse("get vehicle speed").unwrap();
        assert_eq!(intent.tool_name, "read_pid");
        assert_eq!(intent.tool_args["pid"], "0x0D");
    }

    #[test]
    fn parse_check_coolant_temp() {
        let intent = parse("check coolant temp").unwrap();
        assert_eq!(intent.tool_name, "read_pid");
        assert_eq!(intent.tool_args["pid"], "0x05");
    }

    #[test]
    fn parse_read_pid_hex() {
        let intent = parse("read pid 0x0C").unwrap();
        assert_eq!(intent.tool_name, "read_pid");
        assert_eq!(intent.tool_args["pid"], "0X0C");
    }

    #[test]
    fn parse_show_throttle() {
        let intent = parse("show throttle position").unwrap();
        assert_eq!(intent.tool_name, "read_pid");
        assert_eq!(intent.tool_args["pid"], "0x11");
    }

    #[test]
    fn parse_read_fuel_level() {
        let intent = parse("read fuel level").unwrap();
        assert_eq!(intent.tool_name, "read_pid");
        assert_eq!(intent.tool_args["pid"], "0x2F");
    }

    // ── CAN monitor ─────────────────────────────────────────────

    #[test]
    fn parse_monitor_can() {
        let intent = parse("monitor CAN bus traffic").unwrap();
        assert_eq!(intent.tool_name, "can_monitor");
        assert_eq!(intent.tool_args["duration_secs"], 10);
    }

    #[test]
    fn parse_sniff_can_with_duration() {
        let intent = parse("sniff CAN bus for 30 seconds").unwrap();
        assert_eq!(intent.tool_name, "can_monitor");
        assert_eq!(intent.tool_args["duration_secs"], 30);
    }

    // ── Log commands ────────────────────────────────────────────

    #[test]
    fn parse_search_logs() {
        let intent = parse("search logs for connection timeout").unwrap();
        assert_eq!(intent.tool_name, "search_logs");
        assert_eq!(intent.tool_args["query"], "connection timeout");
        assert!(intent.confidence >= 0.9);
    }

    #[test]
    fn parse_search_logs_no_query() {
        let intent = parse("search logs").unwrap();
        assert_eq!(intent.tool_name, "search_logs");
        assert_eq!(intent.tool_args["query"], "error");
        assert!(intent.confidence < 0.9); // Lower confidence without explicit query
    }

    #[test]
    fn parse_analyze_errors() {
        let intent = parse("analyze errors in the logs").unwrap();
        assert_eq!(intent.tool_name, "analyze_errors");
    }

    #[test]
    fn parse_log_stats() {
        let intent = parse("show log statistics").unwrap();
        assert_eq!(intent.tool_name, "log_stats");
    }

    #[test]
    fn parse_tail_logs() {
        let intent = parse("tail logs").unwrap();
        assert_eq!(intent.tool_name, "tail_logs");
        assert_eq!(intent.tool_args["lines"], 50);
    }

    #[test]
    fn parse_recent_logs_with_count() {
        let intent = parse("show recent logs 200").unwrap();
        assert_eq!(intent.tool_name, "tail_logs");
        assert_eq!(intent.tool_args["lines"], 200);
    }

    // ── Journal commands ──────────────────────────────────────

    #[test]
    fn parse_journal_for_service() {
        let intent = parse("show journal for nginx.service").unwrap();
        assert_eq!(intent.tool_name, "query_journal");
        assert_eq!(intent.tool_args["unit"], "nginx.service");
    }

    #[test]
    fn parse_journalctl_service() {
        let intent = parse("journalctl sshd").unwrap();
        assert_eq!(intent.tool_name, "query_journal");
        assert_eq!(intent.tool_args["unit"], "sshd");
    }

    #[test]
    fn parse_service_logs_for() {
        let intent = parse("service logs for docker.service").unwrap();
        assert_eq!(intent.tool_name, "query_journal");
        assert_eq!(intent.tool_args["unit"], "docker.service");
    }

    #[test]
    fn parse_systemd_logs_fallback() {
        let intent = parse("show systemd logs").unwrap();
        assert_eq!(intent.tool_name, "query_journal");
        // No explicit service → falls back to systemd-journald.service
        assert_eq!(intent.tool_args["unit"], "systemd-journald.service");
    }

    // ── Unrecognized ────────────────────────────────────────────

    #[test]
    fn unrecognized_returns_none() {
        assert!(parse("hello world").is_none());
        assert!(parse("what time is it").is_none());
        assert!(parse("deploy the application").is_none());
    }

    // ── Helper tests ────────────────────────────────────────────

    #[test]
    fn extract_hex_value_0x() {
        assert_eq!(extract_hex_value("read pid 0x0C"), Some("0X0C".into()));
    }

    #[test]
    fn extract_hex_value_decimal() {
        assert_eq!(extract_hex_value("read pid 12"), Some("0x0C".into()));
    }

    #[test]
    fn extract_duration_seconds() {
        assert_eq!(extract_duration("monitor for 30 seconds"), Some(30));
    }

    #[test]
    fn extract_duration_shorthand() {
        assert_eq!(extract_duration("capture 15s"), Some(15));
    }

    #[test]
    fn extract_search_query_for() {
        assert_eq!(
            extract_search_query("search logs for connection refused"),
            Some("connection refused")
        );
    }

    #[test]
    fn extract_service_journal_for() {
        assert_eq!(
            extract_service_name("show journal for nginx.service"),
            Some("nginx.service")
        );
    }

    #[test]
    fn extract_service_journalctl() {
        assert_eq!(extract_service_name("journalctl sshd"), Some("sshd"));
    }

    #[test]
    fn extract_service_logs_for() {
        assert_eq!(
            extract_service_name("service logs for docker.service"),
            Some("docker.service")
        );
    }

    #[test]
    fn extract_service_none() {
        assert_eq!(extract_service_name("show systemd logs"), None);
    }
}
