//! Ollama local inference client for on-device NL command parsing.
//!
//! Calls the local Ollama HTTP API (`/api/chat`) to parse natural-language
//! operator commands into structured intents. Supports three action types:
//! - **tool**: Invoke one of 10 registered diagnostic tools
//! - **shell**: Execute a safe system command on the device
//! - **reply**: Return a conversational response (no execution)

use serde::{Deserialize, Serialize};
use zc_protocol::commands::{ActionKind, ParsedIntent};

/// System prompt teaching three action types: tool, shell, reply.
const SYSTEM_PROMPT: &str = r#"You are an AI agent running on an IoT edge device in a vehicle fleet. You can do three things:

## Action 1: tool — Invoke a diagnostic tool
Use this for vehicle diagnostics and log analysis. Available tools:

1. read_dtcs — Read diagnostic trouble codes from the vehicle ECU. Args: {}
2. read_vin — Read the Vehicle Identification Number. Args: {}
3. read_freeze — Read freeze frame data. Args: {}
4. read_pid — Read an OBD-II sensor value. Args: {"pid": "0x0C"} (0x0C=RPM, 0x0D=speed, 0x05=coolant temp, 0x11=throttle, 0x2F=fuel level, 0x04=engine load, 0x0F=intake temp, 0x0E=timing advance)
5. can_monitor — Monitor raw CAN bus traffic. Args: {"duration_secs": 10}
6. search_logs — Search device logs. Args: {"path": "/var/log/syslog", "query": "error"}
7. analyze_errors — Analyze error patterns in logs. Args: {"path": "/var/log/syslog"}
8. log_stats — Get log statistics. Args: {"path": "/var/log/syslog"}
9. tail_logs — Show recent log entries. Args: {"path": "/var/log/syslog", "lines": 50}
10. query_journal — Query systemd journal for a service. Args: {"unit": "nginx.service", "lines": 50}

Response format: {"action": "tool", "tool_name": "<name>", "tool_args": {<args>}, "confidence": <0.0-1.0>}

## Action 2: shell — Run a system command
Use this for system info queries like CPU temperature, disk space, memory, network status, uptime, etc. Only read-only commands are safe — the device enforces an allowlist.

IMPORTANT: Use simple single commands only. Do NOT use pipes (|), semicolons (;), redirects (> <), backticks, $(), or && — these are blocked. Use command flags instead.

Response format: {"action": "shell", "command": "<single command with flags>", "confidence": <0.0-1.0>}

Examples:
- "what's the CPU temperature?" → {"action": "shell", "command": "cat /sys/class/thermal/thermal_zone0/temp", "confidence": 0.9}
- "how much disk space is left?" → {"action": "shell", "command": "df -h", "confidence": 0.95}
- "show memory usage" → {"action": "shell", "command": "free -h", "confidence": 0.95}
- "what processes are running?" → {"action": "shell", "command": "ps aux", "confidence": 0.9}
- "show network interfaces" → {"action": "shell", "command": "ip addr", "confidence": 0.9}
- "system uptime?" → {"action": "shell", "command": "uptime", "confidence": 0.95}
- "kernel version?" → {"action": "shell", "command": "uname -a", "confidence": 0.95}
- "list files in /tmp" → {"action": "shell", "command": "ls -la /tmp", "confidence": 0.9}
- "CPU info?" → {"action": "shell", "command": "lscpu", "confidence": 0.95}
- "journalctl status?" → {"action": "shell", "command": "journalctl -n 20 --no-pager", "confidence": 0.9}

## Action 3: reply — Conversational response
Use this for greetings, questions about yourself, or anything that doesn't need a tool or shell command.

Response format: {"action": "reply", "message": "<your response>", "confidence": 1.0}

Examples:
- "how are you?" → {"action": "reply", "message": "I'm operational and monitoring the fleet. All systems nominal.", "confidence": 1.0}
- "hello" → {"action": "reply", "message": "Hello! I'm the fleet agent for this device. How can I help?", "confidence": 1.0}
- "what can you do?" → {"action": "reply", "message": "I can read vehicle diagnostics (DTCs, PIDs, VIN), analyze logs, run system commands, and monitor CAN bus traffic.", "confidence": 1.0}

## Rules
- Respond with ONLY a JSON object (no markdown, no explanation)
- Be generous in interpretation — operators use casual language
- For vehicle/diagnostic queries → action: tool
- For ANY log-related queries (show logs, tail logs, search logs, system logs, syslog, recent logs) → action: tool (use tail_logs, search_logs, analyze_errors, or log_stats)
- For journal/service log queries (e.g. "show nginx logs", "journal for sshd") → action: tool (use query_journal)
- For system/OS queries (CPU, memory, disk, network, processes) → action: shell
- For conversation/greetings → action: reply
- When unsure, prefer "reply" with a helpful message over returning nothing"#;

/// Known tool names for validation. Must match the tools in SYSTEM_PROMPT.
const KNOWN_TOOLS: &[&str] = &[
    "read_dtcs",
    "read_vin",
    "read_freeze",
    "read_pid",
    "can_monitor",
    "search_logs",
    "analyze_errors",
    "log_stats",
    "tail_logs",
    "query_journal",
];

/// Log tools that require a "path" argument.
const LOG_TOOLS: &[&str] = &["search_logs", "analyze_errors", "log_stats", "tail_logs"];

/// Default log path when LLM omits it.
const DEFAULT_LOG_PATH: &str = "/var/log/syslog";

/// Shell metacharacters to strip from LLM-generated commands.
const SHELL_METACHAR_PREFIXES: &[char] = &['|', ';', '`', '>', '<', '&', '\n', '\r'];

/// Minimum confidence threshold — below this we treat as "no match".
const MIN_CONFIDENCE: f64 = 0.3;

/// Strip everything from the first shell metacharacter onward.
/// LLMs (especially small ones like phi3) often add pipes despite instructions.
/// Since we use `tokio::process::Command` (no shell), pipes don't work anyway.
fn sanitize_shell_command(cmd: &str) -> String {
    let cmd = cmd.trim();
    // Find the earliest metacharacter position
    let cut = cmd
        .find("$(")
        .into_iter()
        .chain(SHELL_METACHAR_PREFIXES.iter().filter_map(|&c| cmd.find(c)))
        .min();
    match cut {
        Some(pos) if pos > 0 => cmd[..pos].trim().to_string(),
        Some(_) => String::new(), // metachar at start — nothing useful
        None => cmd.to_string(),
    }
}

/// Inject default `path` for log tools if the LLM omitted it.
fn ensure_log_tool_path(tool_name: &str, mut args: serde_json::Value) -> serde_json::Value {
    if LOG_TOOLS.contains(&tool_name) {
        if let Some(obj) = args.as_object_mut() {
            if !obj.contains_key("path") {
                obj.insert(
                    "path".to_string(),
                    serde_json::Value::String(DEFAULT_LOG_PATH.to_string()),
                );
            }
        } else {
            // args wasn't an object — replace with default
            args = serde_json::json!({ "path": DEFAULT_LOG_PATH });
        }
    }
    args
}

/// Configuration for the local Ollama inference endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct OllamaConfig {
    /// Ollama HTTP API base URL.
    #[serde(default = "default_host")]
    pub host: String,
    /// Model to use for inference.
    #[serde(default = "default_model")]
    pub model: String,
    /// Request timeout in seconds.
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// Whether local inference is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_host() -> String {
    "http://localhost:11434".into()
}
fn default_model() -> String {
    "phi3:mini".into()
}
fn default_timeout_secs() -> u64 {
    5
}
fn default_enabled() -> bool {
    true
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            model: default_model(),
            timeout_secs: default_timeout_secs(),
            enabled: default_enabled(),
        }
    }
}

/// Ollama chat API request body.
#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    format: &'a str,
    stream: bool,
}

/// A single message in the chat request.
#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

/// Ollama chat API response (only fields we need).
#[derive(Deserialize)]
struct ChatResponse {
    message: Option<ResponseMessage>,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}

/// Raw LLM output before validation — supports all three action types.
#[derive(Deserialize)]
struct RawIntent {
    /// Action type: "tool", "shell", or "reply". Defaults to "tool" for backward compat.
    #[serde(default = "default_action")]
    action: String,
    /// Tool name (for action=tool) — may be null.
    tool_name: Option<String>,
    /// Tool arguments (for action=tool).
    #[serde(default)]
    tool_args: serde_json::Value,
    /// Shell command string (for action=shell).
    command: Option<String>,
    /// Conversational reply (for action=reply).
    message: Option<String>,
    /// Confidence score.
    #[serde(default)]
    confidence: f64,
}

fn default_action() -> String {
    "tool".into()
}

/// Client for the local Ollama inference endpoint.
pub struct OllamaClient {
    client: reqwest::Client,
    config: OllamaConfig,
}

impl OllamaClient {
    pub fn new(config: OllamaConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .build()
            .expect("failed to build reqwest client");
        Self { client, config }
    }

    /// Parse a natural-language command into a `ParsedIntent`.
    ///
    /// Supports three action types:
    /// - `tool`: validates tool_name against KNOWN_TOOLS
    /// - `shell`: validates command field exists
    /// - `reply`: validates message field exists
    ///
    /// Returns `None` if Ollama is unreachable, returns garbage, or
    /// confidence is below threshold.
    pub async fn parse(&self, text: &str) -> Option<ParsedIntent> {
        let url = format!("{}/api/chat", self.config.host);

        let body = ChatRequest {
            model: &self.config.model,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: SYSTEM_PROMPT,
                },
                ChatMessage {
                    role: "user",
                    content: text,
                },
            ],
            format: "json",
            stream: false,
        };

        let response = match self.client.post(&url).json(&body).send().await {
            Ok(resp) => resp,
            Err(e) => {
                tracing::warn!(error = %e, "ollama request failed");
                return None;
            }
        };

        if !response.status().is_success() {
            tracing::warn!(status = %response.status(), "ollama returned non-200");
            return None;
        }

        let chat_resp: ChatResponse = match response.json().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "failed to parse ollama response body");
                return None;
            }
        };

        let content = chat_resp.message?.content;

        let raw: RawIntent = match serde_json::from_str(&content) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, content = %content, "ollama returned invalid JSON");
                return None;
            }
        };

        // Route based on action type
        match raw.action.as_str() {
            "tool" => self.validate_tool_intent(raw),
            "shell" => self.validate_shell_intent(raw),
            "reply" => self.validate_reply_intent(raw),
            other => {
                tracing::warn!(action = %other, "ollama returned unknown action type");
                // Graceful fallback 1: action is itself a known tool name
                // (phi3 sometimes puts tool_name in the action field)
                if KNOWN_TOOLS.contains(&other) {
                    self.validate_tool_intent(RawIntent {
                        action: "tool".into(),
                        tool_name: Some(other.to_string()),
                        ..raw
                    })
                // Graceful fallback 2: separate tool_name field exists
                } else if raw.tool_name.is_some() {
                    self.validate_tool_intent(RawIntent {
                        action: "tool".into(),
                        ..raw
                    })
                } else {
                    None
                }
            }
        }
    }

    /// Validate a tool action: tool_name must be known, confidence above threshold.
    /// Injects default `path` for log tools when missing (phi3 sometimes omits it).
    fn validate_tool_intent(&self, raw: RawIntent) -> Option<ParsedIntent> {
        let tool_name = raw.tool_name?;
        if !KNOWN_TOOLS.contains(&tool_name.as_str()) {
            tracing::warn!(tool_name = %tool_name, "ollama returned unknown tool");
            return None;
        }

        if raw.confidence < MIN_CONFIDENCE {
            tracing::debug!(
                confidence = raw.confidence,
                tool_name = %tool_name,
                "ollama confidence below threshold"
            );
            return None;
        }

        // Log tools require a "path" argument — inject default if missing
        let tool_args = ensure_log_tool_path(&tool_name, raw.tool_args);

        Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name,
            tool_args,
            confidence: raw.confidence,
        })
    }

    /// Validate a shell action: command field must be present and non-empty.
    /// Sanitizes commands by stripping anything from the first shell metacharacter
    /// onward, since phi3 sometimes generates piped commands despite instructions.
    fn validate_shell_intent(&self, raw: RawIntent) -> Option<ParsedIntent> {
        let command = raw.command.filter(|c| !c.trim().is_empty())?;

        // Strip from first metacharacter — LLMs sometimes add pipes/redirects
        // even when told not to. We only run the base command.
        let sanitized = sanitize_shell_command(&command);
        if sanitized.is_empty() {
            return None;
        }

        if raw.confidence < MIN_CONFIDENCE {
            tracing::debug!(
                confidence = raw.confidence,
                "shell intent confidence too low"
            );
            return None;
        }

        if sanitized != command {
            tracing::info!(
                original = %command,
                sanitized = %sanitized,
                "shell command sanitized (metacharacters stripped)"
            );
        }

        Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: sanitized,
            tool_args: raw.tool_args,
            confidence: raw.confidence,
        })
    }

    /// Validate a reply action: message field must be present and non-empty.
    fn validate_reply_intent(&self, raw: RawIntent) -> Option<ParsedIntent> {
        let message = raw.message.filter(|m| !m.trim().is_empty())?;

        Some(ParsedIntent {
            action: ActionKind::Reply,
            tool_name: String::new(),
            tool_args: serde_json::json!({ "message": message }),
            confidence: raw.confidence.max(1.0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Helper: build an Ollama chat response body.
    fn ollama_response(content: &str) -> serde_json::Value {
        serde_json::json!({
            "model": "phi3:mini",
            "message": {
                "role": "assistant",
                "content": content
            },
            "done": true
        })
    }

    /// Build an OllamaClient pointed at the mock server.
    fn client_for(server: &MockServer) -> OllamaClient {
        OllamaClient::new(OllamaConfig {
            host: server.uri(),
            model: "phi3:mini".into(),
            timeout_secs: 2,
            enabled: true,
        })
    }

    // ── Tool action tests (existing, updated) ────────────────────

    #[tokio::test]
    async fn parse_tool_read_dtcs() {
        let server = MockServer::start().await;
        let body = ollama_response(
            r#"{"action": "tool", "tool_name": "read_dtcs", "tool_args": {}, "confidence": 0.95}"#,
        );
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let intent = client
            .parse("read the diagnostic trouble codes")
            .await
            .unwrap();
        assert_eq!(intent.action, ActionKind::Tool);
        assert_eq!(intent.tool_name, "read_dtcs");
        assert!((intent.confidence - 0.95).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn parse_tool_backward_compat_no_action() {
        // Old-format JSON without "action" field should default to tool
        let server = MockServer::start().await;
        let body =
            ollama_response(r#"{"tool_name": "read_dtcs", "tool_args": {}, "confidence": 0.95}"#);
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let intent = client.parse("read DTCs").await.unwrap();
        assert_eq!(intent.action, ActionKind::Tool);
        assert_eq!(intent.tool_name, "read_dtcs");
    }

    #[tokio::test]
    async fn parse_unknown_tool_returns_none() {
        let server = MockServer::start().await;
        let body = ollama_response(
            r#"{"action": "tool", "tool_name": "self_destruct", "tool_args": {}, "confidence": 0.99}"#,
        );
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        assert!(client.parse("destroy everything").await.is_none());
    }

    #[tokio::test]
    async fn parse_low_confidence_tool() {
        let server = MockServer::start().await;
        let body = ollama_response(
            r#"{"action": "tool", "tool_name": "read_pid", "tool_args": {"pid": "0x0C"}, "confidence": 0.1}"#,
        );
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        assert!(client.parse("maybe something").await.is_none());
    }

    // ── Shell action tests ───────────────────────────────────────

    #[tokio::test]
    async fn parse_shell_command() {
        let server = MockServer::start().await;
        let body =
            ollama_response(r#"{"action": "shell", "command": "sensors", "confidence": 0.9}"#);
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let intent = client.parse("what's the CPU temperature?").await.unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "sensors");
    }

    #[tokio::test]
    async fn parse_shell_df() {
        let server = MockServer::start().await;
        let body =
            ollama_response(r#"{"action": "shell", "command": "df -h", "confidence": 0.95}"#);
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let intent = client.parse("how much disk space?").await.unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "df -h");
    }

    #[tokio::test]
    async fn parse_shell_empty_command_returns_none() {
        let server = MockServer::start().await;
        let body = ollama_response(r#"{"action": "shell", "command": "", "confidence": 0.9}"#);
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        assert!(client.parse("do something").await.is_none());
    }

    // ── Reply action tests ───────────────────────────────────────

    #[tokio::test]
    async fn parse_reply() {
        let server = MockServer::start().await;
        let body = ollama_response(
            r#"{"action": "reply", "message": "Hello! I'm the fleet agent.", "confidence": 1.0}"#,
        );
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let intent = client.parse("hello").await.unwrap();
        assert_eq!(intent.action, ActionKind::Reply);
        assert_eq!(intent.tool_args["message"], "Hello! I'm the fleet agent.");
    }

    #[tokio::test]
    async fn parse_reply_empty_message_returns_none() {
        let server = MockServer::start().await;
        let body = ollama_response(r#"{"action": "reply", "message": "", "confidence": 1.0}"#);
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        assert!(client.parse("...").await.is_none());
    }

    // ── Unknown action fallback ──────────────────────────────────

    #[tokio::test]
    async fn parse_unknown_action_with_tool_name_falls_back() {
        let server = MockServer::start().await;
        let body = ollama_response(
            r#"{"action": "magic", "tool_name": "read_vin", "tool_args": {}, "confidence": 0.8}"#,
        );
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let intent = client.parse("read vin").await.unwrap();
        assert_eq!(intent.action, ActionKind::Tool);
        assert_eq!(intent.tool_name, "read_vin");
    }

    #[tokio::test]
    async fn parse_action_is_tool_name_falls_back() {
        // phi3 sometimes puts the tool name in the action field
        let server = MockServer::start().await;
        let body = ollama_response(
            r#"{"action": "tail_logs", "tool_args": {"path": "/var/log/syslog", "lines": 50}, "confidence": 0.9}"#,
        );
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        let intent = client.parse("show system logs").await.unwrap();
        assert_eq!(intent.action, ActionKind::Tool);
        assert_eq!(intent.tool_name, "tail_logs");
    }

    #[tokio::test]
    async fn parse_unknown_action_no_tool_returns_none() {
        let server = MockServer::start().await;
        let body = ollama_response(r#"{"action": "dance", "confidence": 0.5}"#);
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        assert!(client.parse("do a dance").await.is_none());
    }

    // ── Null tool_name (old format) now treated as no-match ──────

    #[tokio::test]
    async fn parse_null_tool_name_returns_none() {
        let server = MockServer::start().await;
        let body = ollama_response(r#"{"tool_name": null, "tool_args": {}, "confidence": 0.0}"#);
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        assert!(client.parse("bake a pizza").await.is_none());
    }

    // ── Error handling ───────────────────────────────────────────

    #[tokio::test]
    async fn parse_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(10)))
            .mount(&server)
            .await;

        let client = client_for(&server);
        assert!(client.parse("read DTCs").await.is_none());
    }

    #[tokio::test]
    async fn parse_invalid_json() {
        let server = MockServer::start().await;
        let body = ollama_response("this is not json at all");
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let client = client_for(&server);
        assert!(client.parse("read DTCs").await.is_none());
    }

    // ── Helper function tests ─────────────────────────────────────

    #[test]
    fn sanitize_strips_pipe() {
        assert_eq!(
            sanitize_shell_command("cat /sys/class/thermal/thermal_zone0/temp | awk '{print}'"),
            "cat /sys/class/thermal/thermal_zone0/temp"
        );
    }

    #[test]
    fn sanitize_strips_semicolon() {
        assert_eq!(sanitize_shell_command("ls /tmp; rm -rf /"), "ls /tmp");
    }

    #[test]
    fn sanitize_strips_redirect() {
        assert_eq!(sanitize_shell_command("echo hi > /etc/passwd"), "echo hi");
    }

    #[test]
    fn sanitize_clean_command_unchanged() {
        assert_eq!(sanitize_shell_command("df -h"), "df -h");
    }

    #[test]
    fn sanitize_leading_metachar_returns_empty() {
        assert_eq!(sanitize_shell_command("|cat"), "");
    }

    #[test]
    fn ensure_path_injects_default_for_log_tool() {
        let args = serde_json::json!({});
        let result = ensure_log_tool_path("tail_logs", args);
        assert_eq!(result["path"], "/var/log/syslog");
    }

    #[test]
    fn ensure_path_preserves_existing() {
        let args = serde_json::json!({"path": "/var/log/app.log"});
        let result = ensure_log_tool_path("tail_logs", args);
        assert_eq!(result["path"], "/var/log/app.log");
    }

    #[test]
    fn ensure_path_skips_non_log_tool() {
        let args = serde_json::json!({});
        let result = ensure_log_tool_path("read_dtcs", args);
        assert!(result.get("path").is_none());
    }

    // ── Config tests ─────────────────────────────────────────────

    #[tokio::test]
    async fn config_defaults() {
        let config = OllamaConfig::default();
        assert_eq!(config.host, "http://localhost:11434");
        assert_eq!(config.model, "phi3:mini");
        assert_eq!(config.timeout_secs, 5);
        assert!(config.enabled);
    }

    #[tokio::test]
    async fn config_from_toml() {
        let toml_str = r#"
host = "http://192.168.1.50:11434"
model = "gemma:2b"
timeout_secs = 10
enabled = false
"#;
        let config: OllamaConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.host, "http://192.168.1.50:11434");
        assert_eq!(config.model, "gemma:2b");
        assert_eq!(config.timeout_secs, 10);
        assert!(!config.enabled);
    }
}
