//! AWS Bedrock edge inference engine — Converse API for cloud LLM fallback.
//!
//! Feature-gated behind `features = ["bedrock"]` to keep the default binary
//! small for constrained ARM devices. Adapted from the cloud-side
//! `zc-cloud-api/src/inference/bedrock.rs` with edge-specific adaptations
//! (returns `ParsedIntent` directly, integrates with `EdgeInferenceEngine` trait).

use std::time::Duration;

use async_trait::async_trait;
use aws_sdk_bedrockruntime::Client as BedrockClient;
use aws_sdk_bedrockruntime::types::{ContentBlock, ConversationRole, Message, SystemContentBlock};
use serde::Deserialize;
use zc_protocol::commands::{ActionKind, ParsedIntent, RecoverySource};

use crate::config::BedrockConfig;
use crate::inference::{EdgeInferenceEngine, sanitize_shell_command};
use crate::recovery::RecoveryContext;

/// System prompt listing all 13 tools plus shell and reply action types.
/// Identical to the cloud bedrock.rs prompt — same tools, same rules.
const SYSTEM_PROMPT: &str = r#"You are an AI agent running on an IoT edge device in a vehicle fleet. You can do three things:

## Action 1: tool — Invoke a diagnostic tool
Use this for vehicle diagnostics and log analysis. Available tools:

1. read_dtcs — Read diagnostic trouble codes from the vehicle ECU. Args: {}
2. read_vin — Read the Vehicle Identification Number. Args: {}
3. read_freeze — Read freeze frame data. Args: {}
4. read_pid — Read an OBD-II sensor value. Args: {"pid": "0x0C"} (0x0C=RPM, 0x0D=speed, 0x05=coolant temp, 0x11=throttle, 0x2F=fuel level, 0x04=engine load, 0x0F=intake temp, 0x0E=timing advance)
5. can_monitor — Monitor raw CAN bus traffic. Args: {"duration_secs": 10}
6. read_uds_dtcs — Read DTCs from a UDS ECU (Hella BCR/BCF). Args: {"ecu": "BCR"} or {"ecu": "BCF"}
7. read_uds_did — Read a Data Identifier from a UDS ECU. Args: {"ecu": "BCR"} (reads all known DIDs) or {"ecu": "BCR", "did": 64773} (specific DID 0xFD05)
8. uds_session_control — Control diagnostic session on a UDS ECU. Args: {"ecu": "BCR", "session": "extended"} or {"ecu": "BCR", "tester_present": true}
9. search_logs — Search device logs. Args: {"path": "/var/log/syslog", "query": "error"}
10. analyze_errors — Analyze error patterns in logs. Args: {"path": "/var/log/syslog"}
11. log_stats — Get log statistics. Args: {"path": "/var/log/syslog"}
12. tail_logs — Show recent log entries. Args: {"path": "/var/log/syslog", "lines": 50}
13. query_journal — Query systemd journal for a service. Args: {"unit": "nginx.service", "lines": 50}

Response format: {"action": "tool", "tool_name": "<name>", "tool_args": {<args>}, "confidence": <0.0-1.0>}

## Action 2: shell — Run a system command
Use this for system info queries like CPU temperature, disk space, memory, network status, uptime, etc. Only read-only commands are safe — the device enforces an allowlist.

IMPORTANT: Use simple single commands only. Do NOT use pipes (|), semicolons (;), redirects (> <), backticks, $(), or && — these are blocked.

Response format: {"action": "shell", "command": "<single command with flags>", "confidence": <0.0-1.0>}

Examples:
- "what's the CPU temperature?" → {"action": "shell", "command": "cat /sys/class/thermal/thermal_zone0/temp", "confidence": 0.9}
- "how much disk space is left?" → {"action": "shell", "command": "df -h", "confidence": 0.95}
- "show memory usage" → {"action": "shell", "command": "free -h", "confidence": 0.95}
- "system uptime?" → {"action": "shell", "command": "uptime", "confidence": 0.95}
- "kernel version?" → {"action": "shell", "command": "uname -a", "confidence": 0.95}
- "which app is consuming CPU?" → {"action": "shell", "command": "top -b -n 1", "confidence": 0.9}
- "show hardware sensors" → {"action": "shell", "command": "sensors", "confidence": 0.9}
- "show kernel messages" → {"action": "shell", "command": "dmesg --level=err,warn -T", "confidence": 0.9}
- "what ports are open?" → {"action": "shell", "command": "ss -tulnp", "confidence": 0.9}
- "show block devices" → {"action": "shell", "command": "lsblk", "confidence": 0.95}
- "list running services" → {"action": "shell", "command": "systemctl list-units --type=service --state=running --no-pager", "confidence": 0.9}

IMPORTANT: `top` MUST use `-b -n 1` (batch mode). `dmesg` should use `-T --level=err,warn`.

## Action 3: reply — Conversational response
Use this for greetings, questions about yourself, or anything that doesn't need a tool or shell command.

Response format: {"action": "reply", "message": "<your response>", "confidence": 1.0}

## Rules
- Respond with ONLY a JSON object (no markdown, no explanation)
- Be generous in interpretation — operators use casual language
- For vehicle/diagnostic queries → action: tool
- For system/OS queries → action: shell
- For conversation/greetings → action: reply
- When unsure, prefer "reply" with a helpful message over returning nothing"#;

/// Known tool names for validation.
const KNOWN_TOOLS: &[&str] = &[
    "read_dtcs",
    "read_vin",
    "read_freeze",
    "read_pid",
    "can_monitor",
    "read_uds_dtcs",
    "read_uds_did",
    "uds_session_control",
    "search_logs",
    "analyze_errors",
    "log_stats",
    "tail_logs",
    "query_journal",
];

/// Minimum confidence threshold — below this we treat as "no match".
const MIN_CONFIDENCE: f64 = 0.3;

/// Expected JSON shape from the LLM — supports all three action types.
#[derive(Debug, Deserialize)]
struct LlmResponse {
    #[serde(default = "default_action")]
    action: String,
    tool_name: Option<String>,
    #[serde(default)]
    tool_args: serde_json::Value,
    command: Option<String>,
    message: Option<String>,
    #[serde(default)]
    confidence: f64,
}

fn default_action() -> String {
    "tool".into()
}

/// Extract JSON from LLM output that may be wrapped in markdown code blocks.
fn extract_json(text: &str) -> &str {
    let trimmed = text.trim();

    // Try ```json ... ``` first
    if let Some(start) = trimmed.find("```json") {
        let after_fence = &trimmed[start + 7..];
        if let Some(end) = after_fence.find("```") {
            return after_fence[..end].trim();
        }
    }

    // Try ``` ... ```
    if let Some(start) = trimmed.find("```") {
        let after_fence = &trimmed[start + 3..];
        if let Some(end) = after_fence.find("```") {
            return after_fence[..end].trim();
        }
    }

    // Assume raw JSON
    trimmed
}

/// AWS Bedrock inference engine for edge devices.
///
/// Uses the model-agnostic Converse API (works with Nova Lite, Claude, etc.).
/// Placed in the engine chain after Ollama (free/fast) and before fallback.
pub struct EdgeBedrockEngine {
    client: BedrockClient,
    config: BedrockConfig,
}

impl EdgeBedrockEngine {
    /// Create a new engine, loading AWS credentials from the environment.
    pub async fn new(config: BedrockConfig) -> Self {
        let aws_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new(config.region.clone()))
            .load()
            .await;
        let client = BedrockClient::new(&aws_config);
        Self { client, config }
    }

    /// Call the Bedrock Converse API and parse the response into a `ParsedIntent`.
    async fn call_converse(&self, text: &str) -> Option<ParsedIntent> {
        self.call_converse_with_prompt(SYSTEM_PROMPT, text).await
    }

    /// Call Converse with a custom system prompt (used for recovery suggestions).
    pub async fn call_converse_with_prompt(
        &self,
        system_prompt: &str,
        user_text: &str,
    ) -> Option<ParsedIntent> {
        let user_message = Message::builder()
            .role(ConversationRole::User)
            .content(ContentBlock::Text(user_text.to_string()))
            .build()
            .map_err(|e| tracing::warn!(error = %e, "failed to build bedrock message"))
            .ok()?;

        let response = self
            .client
            .converse()
            .model_id(&self.config.model_id)
            .system(SystemContentBlock::Text(system_prompt.to_string()))
            .messages(user_message)
            .send()
            .await
            .map_err(|e| tracing::warn!(error = %e, "bedrock converse API error"))
            .ok()?;

        // Extract text from response
        let output = response.output()?;
        let text_content = match output {
            aws_sdk_bedrockruntime::types::ConverseOutput::Message(msg) => {
                msg.content().iter().find_map(|block| {
                    if let ContentBlock::Text(t) = block {
                        Some(t.clone())
                    } else {
                        None
                    }
                })
            }
            _ => None,
        };

        let raw_text = text_content?;
        let json_str = extract_json(&raw_text);

        let call: LlmResponse = serde_json::from_str(json_str)
            .map_err(|e| {
                tracing::warn!(
                    error = %e,
                    raw = %raw_text,
                    "bedrock returned invalid JSON"
                )
            })
            .ok()?;

        // Route based on action type
        match call.action.as_str() {
            "tool" => validate_tool(call),
            "shell" => validate_shell(call),
            "reply" => validate_reply(call),
            _ => {
                // Fallback: if tool_name present, try tool path
                if call.tool_name.is_some() {
                    validate_tool(call)
                } else {
                    None
                }
            }
        }
    }
}

/// Validate a tool action: tool_name must be known, confidence above threshold.
fn validate_tool(call: LlmResponse) -> Option<ParsedIntent> {
    let tool_name = call.tool_name?;

    if !KNOWN_TOOLS.contains(&tool_name.as_str()) {
        tracing::warn!(tool_name = %tool_name, "bedrock returned unknown tool");
        return None;
    }

    if call.confidence < MIN_CONFIDENCE {
        tracing::debug!(
            confidence = call.confidence,
            tool_name = %tool_name,
            "bedrock confidence below threshold"
        );
        return None;
    }

    Some(ParsedIntent {
        action: ActionKind::Tool,
        tool_name,
        tool_args: call.tool_args,
        confidence: call.confidence,
    })
}

/// Validate a shell action: command must be present and non-empty.
/// Sanitizes commands by stripping shell metacharacters.
fn validate_shell(call: LlmResponse) -> Option<ParsedIntent> {
    let command = call.command.filter(|c| !c.trim().is_empty())?;
    let sanitized = sanitize_shell_command(&command);

    if sanitized.is_empty() {
        return None;
    }

    if call.confidence < MIN_CONFIDENCE {
        tracing::debug!(
            confidence = call.confidence,
            "bedrock shell confidence too low"
        );
        return None;
    }

    if sanitized != command {
        tracing::info!(
            original = %command,
            sanitized = %sanitized,
            "bedrock shell command sanitized"
        );
    }

    Some(ParsedIntent {
        action: ActionKind::Shell,
        tool_name: sanitized,
        tool_args: call.tool_args,
        confidence: call.confidence,
    })
}

/// Validate a reply action: message must be present and non-empty.
fn validate_reply(call: LlmResponse) -> Option<ParsedIntent> {
    let message = call.message.filter(|m| !m.trim().is_empty())?;

    Some(ParsedIntent {
        action: ActionKind::Reply,
        tool_name: String::new(),
        tool_args: serde_json::json!({ "message": message }),
        confidence: 1.0,
    })
}

#[async_trait]
impl EdgeInferenceEngine for EdgeBedrockEngine {
    fn engine_name(&self) -> &str {
        "bedrock"
    }

    fn recovery_source(&self) -> RecoverySource {
        RecoverySource::Bedrock
    }

    async fn parse(&self, text: &str) -> Option<ParsedIntent> {
        let timeout = Duration::from_secs(self.config.timeout_secs);
        match tokio::time::timeout(timeout, self.call_converse(text)).await {
            Ok(result) => result,
            Err(_) => {
                tracing::warn!(
                    timeout_secs = self.config.timeout_secs,
                    "bedrock edge inference timed out"
                );
                None
            }
        }
    }

    async fn suggest_recovery(
        &self,
        ctx: &RecoveryContext,
        timeout: Duration,
    ) -> Option<ParsedIntent> {
        crate::recovery::bedrock_recovery(ctx, self, timeout).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── LlmResponse deserialization ──────────────────────────────

    #[test]
    fn deserialize_tool_response() {
        let json = r#"{"action": "tool", "tool_name": "read_pid", "tool_args": {"pid": "0x0C"}, "confidence": 0.92}"#;
        let resp: LlmResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.action, "tool");
        assert_eq!(resp.tool_name.as_deref(), Some("read_pid"));
        assert_eq!(resp.tool_args["pid"], "0x0C");
        assert!((resp.confidence - 0.92).abs() < f64::EPSILON);
    }

    #[test]
    fn deserialize_shell_response() {
        let json = r#"{"action": "shell", "command": "df -h", "confidence": 0.95}"#;
        let resp: LlmResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.action, "shell");
        assert_eq!(resp.command.as_deref(), Some("df -h"));
    }

    #[test]
    fn deserialize_reply_response() {
        let json = r#"{"action": "reply", "message": "Hello!", "confidence": 1.0}"#;
        let resp: LlmResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.action, "reply");
        assert_eq!(resp.message.as_deref(), Some("Hello!"));
    }

    #[test]
    fn deserialize_null_fields() {
        let json = r#"{"tool_name": null, "tool_args": {}, "confidence": 0.0}"#;
        let resp: LlmResponse = serde_json::from_str(json).unwrap();
        assert!(resp.tool_name.is_none());
        assert!(resp.command.is_none());
        assert!(resp.message.is_none());
        assert_eq!(resp.action, "tool"); // default
    }

    #[test]
    fn deserialize_missing_optional_fields() {
        let json = r#"{"tool_name": "read_vin"}"#;
        let resp: LlmResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.confidence, 0.0);
        assert_eq!(resp.action, "tool");
    }

    // ── extract_json ─────────────────────────────────────────────

    #[test]
    fn extract_json_raw() {
        let input = r#"{"tool_name": "read_dtcs"}"#;
        assert_eq!(extract_json(input), input);
    }

    #[test]
    fn extract_json_markdown_block() {
        let input = "```json\n{\"tool_name\": \"read_vin\"}\n```";
        assert_eq!(extract_json(input), "{\"tool_name\": \"read_vin\"}");
    }

    #[test]
    fn extract_json_surrounding_text() {
        let input = "Here is the result:\n```json\n{\"tool_name\": \"log_stats\"}\n```\nDone.";
        assert_eq!(extract_json(input), "{\"tool_name\": \"log_stats\"}");
    }

    // ── validate_tool ────────────────────────────────────────────

    #[test]
    fn validate_known_tool() {
        let call = LlmResponse {
            action: "tool".into(),
            tool_name: Some("read_dtcs".into()),
            tool_args: serde_json::json!({}),
            command: None,
            message: None,
            confidence: 0.9,
        };
        let intent = validate_tool(call).unwrap();
        assert_eq!(intent.action, ActionKind::Tool);
        assert_eq!(intent.tool_name, "read_dtcs");
    }

    #[test]
    fn validate_unknown_tool_rejected() {
        let call = LlmResponse {
            action: "tool".into(),
            tool_name: Some("hack_ecu".into()),
            tool_args: serde_json::json!({}),
            command: None,
            message: None,
            confidence: 0.9,
        };
        assert!(validate_tool(call).is_none());
    }

    #[test]
    fn validate_low_confidence_rejected() {
        let call = LlmResponse {
            action: "tool".into(),
            tool_name: Some("read_dtcs".into()),
            tool_args: serde_json::json!({}),
            command: None,
            message: None,
            confidence: 0.1,
        };
        assert!(validate_tool(call).is_none());
    }

    #[test]
    fn validate_no_tool_name_rejected() {
        let call = LlmResponse {
            action: "tool".into(),
            tool_name: None,
            tool_args: serde_json::json!({}),
            command: None,
            message: None,
            confidence: 0.9,
        };
        assert!(validate_tool(call).is_none());
    }

    // ── validate_shell ───────────────────────────────────────────

    #[test]
    fn validate_shell_command() {
        let call = LlmResponse {
            action: "shell".into(),
            tool_name: None,
            tool_args: serde_json::json!({}),
            command: Some("df -h".into()),
            message: None,
            confidence: 0.95,
        };
        let intent = validate_shell(call).unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "df -h");
    }

    #[test]
    fn validate_shell_empty_rejected() {
        let call = LlmResponse {
            action: "shell".into(),
            tool_name: None,
            tool_args: serde_json::json!({}),
            command: Some("".into()),
            message: None,
            confidence: 0.9,
        };
        assert!(validate_shell(call).is_none());
    }

    #[test]
    fn validate_shell_sanitizes_pipe() {
        let call = LlmResponse {
            action: "shell".into(),
            tool_name: None,
            tool_args: serde_json::json!({}),
            command: Some("ps aux | grep nginx".into()),
            message: None,
            confidence: 0.9,
        };
        let intent = validate_shell(call).unwrap();
        assert_eq!(intent.tool_name, "ps aux");
    }

    // ── validate_reply ───────────────────────────────────────────

    #[test]
    fn validate_reply_message() {
        let call = LlmResponse {
            action: "reply".into(),
            tool_name: None,
            tool_args: serde_json::json!({}),
            command: None,
            message: Some("Hello there!".into()),
            confidence: 1.0,
        };
        let intent = validate_reply(call).unwrap();
        assert_eq!(intent.action, ActionKind::Reply);
        assert_eq!(intent.tool_args["message"], "Hello there!");
        assert!((intent.confidence - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn validate_reply_empty_rejected() {
        let call = LlmResponse {
            action: "reply".into(),
            tool_name: None,
            tool_args: serde_json::json!({}),
            command: None,
            message: Some("   ".into()),
            confidence: 1.0,
        };
        assert!(validate_reply(call).is_none());
    }

    // ── Config defaults ──────────────────────────────────────────

    #[test]
    fn bedrock_config_defaults() {
        let config = BedrockConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.region, "us-east-1");
        assert_eq!(config.model_id, "us.amazon.nova-lite-v1:0");
        assert_eq!(config.timeout_secs, 15);
    }
}
