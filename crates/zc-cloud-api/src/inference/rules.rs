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

    // ── UDS / Hella ECU commands (must come before generic OBD-II) ─

    // read_uds_dtcs: "read BCR dtcs", "BCR diagnostics", "hella dtcs", "BCF fault codes"
    if let Some(ecu) = extract_ecu_name(lower) {
        if matches_any(lower, &["dtc", "diagnostic", "trouble code", "fault code"]) {
            return Some(ParsedIntent {
                action: ActionKind::Tool,
                tool_name: "read_uds_dtcs".into(),
                tool_args: json!({ "ecu": ecu }),
                confidence: 0.92,
            });
        }

        // read_uds_did: "BCR voltage", "BCR brake light", "BCR status", "read BCR data"
        if matches_any(
            lower,
            &[
                "voltage",
                "brake light",
                "power supply",
                "reprogramming",
                "flash attempt",
                "read data",
                "sensor",
                "status",
            ],
        ) {
            return Some(ParsedIntent {
                action: ActionKind::Tool,
                tool_name: "read_uds_did".into(),
                tool_args: json!({ "ecu": ecu }),
                confidence: 0.90,
            });
        }

        // uds_session_control: "BCR extended session", "BCR tester present"
        if matches_any(lower, &["session", "tester present", "keep alive"]) {
            let tester_present = lower.contains("tester") || lower.contains("keep alive");
            if tester_present {
                return Some(ParsedIntent {
                    action: ActionKind::Tool,
                    tool_name: "uds_session_control".into(),
                    tool_args: json!({ "ecu": ecu, "tester_present": true }),
                    confidence: 0.90,
                });
            }
            let session = if lower.contains("default") {
                "default"
            } else {
                "extended"
            };
            return Some(ParsedIntent {
                action: ActionKind::Tool,
                tool_name: "uds_session_control".into(),
                tool_args: json!({ "ecu": ecu, "session": session }),
                confidence: 0.90,
            });
        }
    }

    // Generic "hella diagnostics" without specific ECU → default to BCR
    if matches_any(lower, &["hella diagnostic", "hella dtc", "hella status"]) {
        return Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "read_uds_dtcs".into(),
            tool_args: json!({ "ecu": "BCR" }),
            confidence: 0.85,
        });
    }

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

    // ── Shell commands (system info queries) ─────────────────

    // IP address / network
    if matches_any(
        lower,
        &["ip address", "ip addr", "network interface", "network info"],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "ip -brief addr".into(),
            tool_args: json!({}),
            confidence: 0.90,
        });
    }

    // CPU temperature
    if matches_any(lower, &["cpu temp", "cpu temperature", "processor temp"]) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "cat /sys/class/thermal/thermal_zone0/temp".into(),
            tool_args: json!({}),
            confidence: 0.85,
        });
    }

    // GPU temperature (Raspberry Pi)
    if matches_any(lower, &["gpu temp", "gpu temperature"]) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "vcgencmd measure_temp".into(),
            tool_args: json!({}),
            confidence: 0.85,
        });
    }

    // Hardware sensors (lm-sensors)
    if matches_any(
        lower,
        &[
            "sensor",
            "temperature sensor",
            "hardware sensor",
            "voltage sensor",
        ],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "sensors".into(),
            tool_args: json!({}),
            confidence: 0.85,
        });
    }

    // Disk space
    if matches_any(
        lower,
        &["disk space", "disk usage", "storage", "free space"],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "df -h".into(),
            tool_args: json!({}),
            confidence: 0.95,
        });
    }

    // Memory usage
    if matches_any(lower, &["memory", "ram", "free mem"]) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "free -h".into(),
            tool_args: json!({}),
            confidence: 0.90,
        });
    }

    // Uptime
    if lower.contains("uptime") {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "uptime".into(),
            tool_args: json!({}),
            confidence: 0.95,
        });
    }

    // Kernel messages / dmesg (must be before kernel version to avoid "kernel" overlap)
    if matches_any(
        lower,
        &[
            "kernel message",
            "kernel log",
            "dmesg",
            "boot message",
            "kernel ring",
        ],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "dmesg --level=err,warn -T".into(),
            tool_args: json!({}),
            confidence: 0.85,
        });
    }

    // Kernel version
    if matches_any(lower, &["kernel version", "kernel", "uname"]) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "uname -a".into(),
            tool_args: json!({}),
            confidence: 0.95,
        });
    }

    // CPU info
    if matches_any(lower, &["cpu info", "processor info", "lscpu"]) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "lscpu".into(),
            tool_args: json!({}),
            confidence: 0.90,
        });
    }

    // CPU usage / top processes
    if matches_any(
        lower,
        &[
            "cpu usage",
            "consuming cpu",
            "which application",
            "highest cpu",
            "memory hog",
        ],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "top -b -n 1".into(),
            tool_args: json!({}),
            confidence: 0.85,
        });
    }

    // Running processes
    if matches_any(lower, &["process", "running process", "what's running"]) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "ps aux".into(),
            tool_args: json!({}),
            confidence: 0.85,
        });
    }

    // Hostname
    if lower.contains("hostname") {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "hostname".into(),
            tool_args: json!({}),
            confidence: 0.95,
        });
    }

    // Machine ID
    if matches_any(
        lower,
        &["machine id", "machine-id", "device id", "device identifier"],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "cat /etc/machine-id".into(),
            tool_args: json!({}),
            confidence: 0.90,
        });
    }

    // DMI / board product info (x86 / UEFI systems)
    if matches_any(
        lower,
        &[
            "product name",
            "board name",
            "board vendor",
            "board info",
            "dmi info",
            "hardware model",
            "product model",
        ],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "cat /sys/class/dmi/id/product_name".into(),
            tool_args: json!({}),
            confidence: 0.85,
        });
    }

    // ARM device model (Raspberry Pi, BeagleBone, Jetson, etc.)
    if matches_any(
        lower,
        &[
            "device model",
            "board model",
            "what board",
            "device tree model",
            "arm model",
        ],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "cat /proc/device-tree/model".into(),
            tool_args: json!({}),
            confidence: 0.85,
        });
    }

    // Internet / download speed — no safe single command available
    if matches_any(
        lower,
        &[
            "internet speed",
            "download speed",
            "upload speed",
            "bandwidth",
        ],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Reply,
            tool_name: String::new(),
            tool_args: json!({
                "message": "Internet speed testing (bandwidth) is not available — \
                    it requires speedtest-cli or iperf3 which are not in the safety allowlist.\n\n\
                    Try these instead:\n\
                    • \"ping test\" — measures network latency to 8.8.8.8 (3 pings)\n\
                    • \"wifi info\" — shows WiFi link speed and signal strength\n\
                    • \"show open ports\" — lists active network connections"
            }),
            confidence: 0.95,
        });
    }

    // WiFi signal strength / wireless info / wifi speed
    if matches_any(
        lower,
        &[
            "wifi signal",
            "wireless signal",
            "signal strength",
            "wifi strength",
            "wifi info",
            "wireless info",
            "wifi speed",
            "wireless speed",
        ],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "iw dev".into(),
            tool_args: json!({}),
            confidence: 0.85,
        });
    }

    // Network latency
    if matches_any(
        lower,
        &[
            "network latency",
            "ping latency",
            "internet latency",
            "ping test",
            "latency test",
            "check latency",
        ],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "ping -c 3 8.8.8.8".into(),
            tool_args: json!({}),
            confidence: 0.85,
        });
    }

    // GPS location / coordinates
    if matches_any(
        lower,
        &[
            "gps location",
            "gps coordinate",
            "where is the device",
            "where is this device",
            "device location",
            "current location",
            "coordinates",
            "latitude",
            "longitude",
            "gps fix",
            "where am i",
        ],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "gpspipe -w -n 3".into(),
            tool_args: json!({}),
            confidence: 0.90,
        });
    }

    // CAN interface details (bitrate, state)
    if matches_any(
        lower,
        &[
            "can interface",
            "can state",
            "can status",
            "can details",
            "can bitrate",
            "can bus state",
        ],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "ip -details link show type can".into(),
            tool_args: json!({}),
            confidence: 0.85,
        });
    }

    // Sockets / open ports / network connections
    if matches_any(
        lower,
        &[
            "socket",
            "open port",
            "listening port",
            "listening",
            "network connection",
            "active connection",
            "netstat",
        ],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "ss -tulnp".into(),
            tool_args: json!({}),
            confidence: 0.85,
        });
    }

    // Directory size
    if matches_any(lower, &["directory size", "folder size", "disk usage by"]) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "du -sh /var/log".into(),
            tool_args: json!({}),
            confidence: 0.80,
        });
    }

    // Block devices / partitions
    if matches_any(
        lower,
        &["block device", "lsblk", "partition", "disk partition"],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "lsblk".into(),
            tool_args: json!({}),
            confidence: 0.90,
        });
    }

    // Current date/time
    if matches_any(
        lower,
        &["current date", "current time", "what time", "what date"],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "date".into(),
            tool_args: json!({}),
            confidence: 0.95,
        });
    }

    // Current user
    if matches_any(
        lower,
        &["current user", "whoami", "who am i", "logged in as"],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "whoami".into(),
            tool_args: json!({}),
            confidence: 0.95,
        });
    }

    // Running services (systemctl)
    if matches_any(
        lower,
        &["service status", "systemctl status", "service running"],
    ) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "systemctl list-units --type=service --state=running --no-pager".into(),
            tool_args: json!({}),
            confidence: 0.85,
        });
    }

    // Ethernet / NIC info ("link speed" omitted — collides with PID "speed" parser)
    if matches_any(lower, &["ethernet", "nic info", "ethtool"]) {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "ethtool eth0".into(),
            tool_args: json!({}),
            confidence: 0.80,
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
    // Keywords must be specific enough to avoid false positives with system queries
    // (e.g., "internet speed" ≠ vehicle speed, "system load" ≠ engine load).
    let named_pids = [
        (&["rpm", "engine speed", "engine rpm"][..], "0x0C"),
        (&["vehicle speed", "car speed"][..], "0x0D"),
        (&["coolant", "coolant temp", "engine temp"][..], "0x05"),
        (&["throttle", "throttle position"][..], "0x11"),
        (&["fuel level", "fuel tank"][..], "0x2F"),
        (&["engine load"][..], "0x04"),
        (&["intake temp", "intake air"][..], "0x0F"),
        (&["timing advance"][..], "0x0E"),
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

/// Extract a known ECU name ("BCR", "BCF") from text (case-insensitive).
fn extract_ecu_name(text: &str) -> Option<&'static str> {
    if text.contains("bcr") {
        Some("BCR")
    } else if text.contains("bcf") {
        Some("BCF")
    } else {
        None
    }
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

    // ── UDS / Hella ECU commands ────────────────────────────────

    #[test]
    fn parse_bcr_dtcs() {
        let intent = parse("read BCR dtcs").unwrap();
        assert_eq!(intent.tool_name, "read_uds_dtcs");
        assert_eq!(intent.tool_args["ecu"], "BCR");
    }

    #[test]
    fn parse_bcf_diagnostic_trouble_codes() {
        let intent = parse("BCF diagnostic trouble codes").unwrap();
        assert_eq!(intent.tool_name, "read_uds_dtcs");
        assert_eq!(intent.tool_args["ecu"], "BCF");
    }

    #[test]
    fn parse_bcr_voltage() {
        let intent = parse("BCR voltage status").unwrap();
        assert_eq!(intent.tool_name, "read_uds_did");
        assert_eq!(intent.tool_args["ecu"], "BCR");
    }

    #[test]
    fn parse_bcr_brake_light() {
        let intent = parse("check BCR brake light").unwrap();
        assert_eq!(intent.tool_name, "read_uds_did");
        assert_eq!(intent.tool_args["ecu"], "BCR");
    }

    #[test]
    fn parse_bcr_reprogramming() {
        let intent = parse("BCR reprogramming attempts").unwrap();
        assert_eq!(intent.tool_name, "read_uds_did");
        assert_eq!(intent.tool_args["ecu"], "BCR");
    }

    #[test]
    fn parse_bcr_extended_session() {
        let intent = parse("BCR extended session").unwrap();
        assert_eq!(intent.tool_name, "uds_session_control");
        assert_eq!(intent.tool_args["ecu"], "BCR");
        assert_eq!(intent.tool_args["session"], "extended");
    }

    #[test]
    fn parse_bcr_tester_present() {
        let intent = parse("BCR tester present").unwrap();
        assert_eq!(intent.tool_name, "uds_session_control");
        assert_eq!(intent.tool_args["ecu"], "BCR");
        assert_eq!(intent.tool_args["tester_present"], true);
    }

    #[test]
    fn parse_hella_diagnostics_defaults_to_bcr() {
        let intent = parse("hella diagnostics").unwrap();
        assert_eq!(intent.tool_name, "read_uds_dtcs");
        assert_eq!(intent.tool_args["ecu"], "BCR");
    }

    #[test]
    fn parse_extract_ecu_name() {
        assert_eq!(extract_ecu_name("read bcr dtcs"), Some("BCR"));
        assert_eq!(extract_ecu_name("bcf voltage"), Some("BCF"));
        assert_eq!(extract_ecu_name("read dtcs"), None);
    }

    // ── Unrecognized ────────────────────────────────────────────

    #[test]
    fn unrecognized_returns_none() {
        assert!(parse("hello world").is_none());
        assert!(parse("bake a pizza").is_none());
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

    // ── Shell command tests ──────────────────────────────────────

    #[test]
    fn parse_ip_address() {
        let intent = parse("whats the ip address of this machine?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "ip -brief addr");
    }

    #[test]
    fn parse_cpu_temperature() {
        let intent = parse("whats the cpu temperature?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(
            intent.tool_name,
            "cat /sys/class/thermal/thermal_zone0/temp"
        );
    }

    #[test]
    fn parse_gpu_temperature() {
        let intent = parse("whats the gpu temperature?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "vcgencmd measure_temp");
    }

    #[test]
    fn parse_disk_space() {
        let intent = parse("how much disk space is left?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "df -h");
    }

    #[test]
    fn parse_memory_usage() {
        let intent = parse("show memory usage").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "free -h");
    }

    #[test]
    fn parse_uptime() {
        let intent = parse("whats the uptime?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "uptime");
    }

    #[test]
    fn parse_kernel_version() {
        let intent = parse("whats the kernel version?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "uname -a");
    }

    #[test]
    fn parse_hostname() {
        let intent = parse("whats the hostname?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "hostname");
    }

    #[test]
    fn parse_cpu_info() {
        let intent = parse("show cpu info").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "lscpu");
    }

    #[test]
    fn parse_machine_id() {
        let intent = parse("what is the machine id?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "cat /etc/machine-id");
    }

    #[test]
    fn parse_device_identifier() {
        let intent = parse("show the device identifier").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "cat /etc/machine-id");
    }

    #[test]
    fn parse_dmi_product_name() {
        let intent = parse("what is the product name?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "cat /sys/class/dmi/id/product_name");
    }

    #[test]
    fn parse_board_vendor() {
        let intent = parse("show board vendor").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "cat /sys/class/dmi/id/product_name");
    }

    #[test]
    fn parse_hardware_model() {
        let intent = parse("what hardware model is this?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "cat /sys/class/dmi/id/product_name");
    }

    #[test]
    fn parse_arm_device_model() {
        let intent = parse("what device model is this board?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "cat /proc/device-tree/model");
    }

    #[test]
    fn parse_device_tree_model() {
        let intent = parse("show device tree model").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "cat /proc/device-tree/model");
    }

    #[test]
    fn parse_wifi_signal() {
        let intent = parse("what is the wifi signal strength?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "iw dev");
    }

    #[test]
    fn parse_wireless_info() {
        let intent = parse("show wireless info").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "iw dev");
    }

    #[test]
    fn parse_wifi_speed() {
        let intent = parse("whats the wifi speed?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "iw dev");
    }

    #[test]
    fn parse_internet_speed_returns_reply() {
        let intent = parse("whats the internet speed?").unwrap();
        assert_eq!(intent.action, ActionKind::Reply);
        let msg = intent.tool_args["message"].as_str().unwrap();
        assert!(msg.contains("not available"), "should explain limitation");
    }

    #[test]
    fn parse_download_speed_returns_reply() {
        let intent = parse("test download speed").unwrap();
        assert_eq!(intent.action, ActionKind::Reply);
    }

    #[test]
    fn parse_network_latency() {
        let intent = parse("check network latency").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "ping -c 3 8.8.8.8");
    }

    #[test]
    fn parse_ping_test() {
        let intent = parse("run a ping test").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "ping -c 3 8.8.8.8");
    }

    #[test]
    fn parse_gps_location() {
        let intent = parse("what is the gps location?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "gpspipe -w -n 3");
    }

    #[test]
    fn parse_where_is_device() {
        let intent = parse("where is the device?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "gpspipe -w -n 3");
    }

    #[test]
    fn parse_coordinates() {
        let intent = parse("show coordinates").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "gpspipe -w -n 3");
    }

    #[test]
    fn parse_can_interface_state() {
        let intent = parse("show CAN interface state").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "ip -details link show type can");
    }

    #[test]
    fn parse_can_bitrate() {
        let intent = parse("what is the CAN bitrate?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "ip -details link show type can");
    }

    // ── New shell command tests (Phase 15) ──────────────────────

    #[test]
    fn parse_cpu_usage() {
        let intent = parse("show cpu usage").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "top -b -n 1");
    }

    #[test]
    fn parse_which_application_consuming_cpu() {
        let intent = parse("which application is consuming lot of CPU?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "top -b -n 1");
    }

    #[test]
    fn parse_sensors() {
        let intent = parse("show hardware sensor readings").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "sensors");
    }

    #[test]
    fn parse_temperature_sensor() {
        let intent = parse("read temperature sensor data").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "sensors");
    }

    #[test]
    fn parse_dmesg() {
        let intent = parse("show kernel messages").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "dmesg --level=err,warn -T");
    }

    #[test]
    fn parse_dmesg_direct() {
        let intent = parse("run dmesg").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "dmesg --level=err,warn -T");
    }

    #[test]
    fn parse_open_ports() {
        let intent = parse("show open ports").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "ss -tulnp");
    }

    #[test]
    fn parse_listening_ports() {
        let intent = parse("what ports are listening?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "ss -tulnp");
    }

    #[test]
    fn parse_directory_size() {
        let intent = parse("what is the directory size of logs?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "du -sh /var/log");
    }

    #[test]
    fn parse_folder_size() {
        let intent = parse("check folder size").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "du -sh /var/log");
    }

    #[test]
    fn parse_lsblk() {
        let intent = parse("show block devices").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "lsblk");
    }

    #[test]
    fn parse_disk_partition() {
        let intent = parse("list disk partitions").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "lsblk");
    }

    #[test]
    fn parse_current_time() {
        let intent = parse("what time is it?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "date");
    }

    #[test]
    fn parse_current_date() {
        let intent = parse("show current date").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "date");
    }

    #[test]
    fn parse_whoami() {
        let intent = parse("whoami").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "whoami");
    }

    #[test]
    fn parse_current_user() {
        let intent = parse("who is the current user?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "whoami");
    }

    #[test]
    fn parse_service_status() {
        let intent = parse("show service status").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(
            intent.tool_name,
            "systemctl list-units --type=service --state=running --no-pager"
        );
    }

    #[test]
    fn parse_services_running() {
        let intent = parse("which services are service running?").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(
            intent.tool_name,
            "systemctl list-units --type=service --state=running --no-pager"
        );
    }

    #[test]
    fn parse_ethtool() {
        let intent = parse("show ethernet info").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "ethtool eth0");
    }

    #[test]
    fn parse_nic_info() {
        let intent = parse("show nic info").unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "ethtool eth0");
    }

    // ── Regression test for original bug ────────────────────────

    #[test]
    fn regression_cpu_consuming_app_matches_top_not_ps() {
        // Original bug: "Which application is consuming lot of CPU?" fell through to
        // Bedrock which generated `top -o %CPU -n 1` (missing -b batch flag).
        // Must match rule-based `top -b -n 1` and NOT fall through to ps aux.
        let intent = parse("Which application is consuming lot of CPU?").unwrap();
        assert_eq!(intent.tool_name, "top -b -n 1");
        assert_ne!(intent.tool_name, "ps aux");
    }
}
