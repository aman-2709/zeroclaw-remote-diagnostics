//! AWS Bedrock inference engine — Converse API for complex queries.
//!
//! Handles the ~20% of queries that the rule-based engine can't match.
//! Uses the model-agnostic Converse API (works with Nova Lite, Claude, etc.).
//! Supports three action types: tool, shell, reply.

use async_trait::async_trait;
use aws_sdk_bedrockruntime::Client as BedrockClient;
use aws_sdk_bedrockruntime::types::{ContentBlock, ConversationRole, Message, SystemContentBlock};
use serde::Deserialize;
use std::time::Duration;
use tokio::time::timeout;

use super::{InferenceEngine, ParseResult};
use zc_protocol::commands::{ActionKind, ParsedIntent};

/// System prompt listing all 10 tools plus shell and reply action types.
///
/// Embedded as a const to avoid pulling zc-canbus-tools/zc-log-tools as dependencies
/// (which would bring in socketcan, regex, etc. into the cloud API binary).
const SYSTEM_PROMPT: &str = r#"You are an AI agent for an IoT fleet management platform. Parse operator commands into one of three actions.

## Action 1: tool — Invoke a diagnostic tool
Available tools:

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

Format: {"action": "tool", "tool_name": "<name>", "tool_args": {<args>}, "confidence": <0.0-1.0>}

## Action 2: shell — Run a system command on the device
For system info queries (CPU temp, disk space, memory, network, uptime, etc.).

Format: {"action": "shell", "command": "<shell command>", "confidence": <0.0-1.0>}

## Action 3: reply — Conversational response
For greetings, questions about capabilities, or non-actionable queries.

Format: {"action": "reply", "message": "<your response>", "confidence": 1.0}

## Rules
- Respond with ONLY a JSON object (no markdown, no explanation)
- Vehicle/diagnostic queries → tool
- System/OS queries → shell
- Conversation/greetings → reply
- Be generous in interpretation — operators use casual language"#;

/// Known tool names for validation.
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

/// Configuration for the Bedrock inference engine.
#[derive(Debug, Clone)]
pub struct BedrockConfig {
    /// Bedrock model ID (e.g., "us.amazon.nova-lite-v1:0").
    pub model_id: String,
    /// Per-request timeout.
    pub timeout: Duration,
}

impl BedrockConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Self {
        let model_id =
            std::env::var("BEDROCK_MODEL_ID").unwrap_or_else(|_| "us.amazon.nova-lite-v1:0".into());
        let timeout_secs: u64 = std::env::var("BEDROCK_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(15);
        Self {
            model_id,
            timeout: Duration::from_secs(timeout_secs),
        }
    }
}

/// Bedrock Converse API inference engine.
pub struct BedrockEngine {
    client: BedrockClient,
    config: BedrockConfig,
}

impl BedrockEngine {
    /// Create a new engine with a pre-built Bedrock client.
    pub fn new(client: BedrockClient, config: BedrockConfig) -> Self {
        Self { client, config }
    }
}

#[async_trait]
impl InferenceEngine for BedrockEngine {
    async fn parse(&self, text: &str) -> Option<ParseResult> {
        let result = timeout(self.config.timeout, self.call_converse(text)).await;

        match result {
            Ok(Ok(Some(intent))) => Some(ParseResult {
                intent,
                tier: "bedrock".into(),
            }),
            Ok(Ok(None)) => {
                tracing::debug!("bedrock returned no match for: {text}");
                None
            }
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "bedrock inference failed");
                None
            }
            Err(_) => {
                tracing::warn!(
                    timeout_secs = self.config.timeout.as_secs(),
                    "bedrock inference timed out"
                );
                None
            }
        }
    }

    fn tier_name(&self) -> &str {
        "bedrock"
    }
}

impl BedrockEngine {
    /// Call the Bedrock Converse API and parse the response.
    async fn call_converse(&self, text: &str) -> anyhow::Result<Option<ParsedIntent>> {
        let user_message = Message::builder()
            .role(ConversationRole::User)
            .content(ContentBlock::Text(text.to_string()))
            .build()
            .map_err(|e| anyhow::anyhow!("failed to build message: {e}"))?;

        let response = self
            .client
            .converse()
            .model_id(&self.config.model_id)
            .system(SystemContentBlock::Text(SYSTEM_PROMPT.to_string()))
            .messages(user_message)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("bedrock converse error: {e}"))?;

        // Extract text from the response
        let output = response
            .output()
            .ok_or_else(|| anyhow::anyhow!("no output in bedrock response"))?;

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

        let Some(raw_text) = text_content else {
            return Ok(None);
        };

        // Parse the JSON from the LLM output
        let json_str = extract_json(&raw_text);
        let call: LlmResponse = serde_json::from_str(json_str)
            .map_err(|e| anyhow::anyhow!("failed to parse bedrock JSON: {e} — raw: {raw_text}"))?;

        // Route based on action type
        match call.action.as_str() {
            "tool" => self.validate_tool(call),
            "shell" => self.validate_shell(call),
            "reply" => self.validate_reply(call),
            _ => {
                // Fallback: if tool_name present, try tool path
                if call.tool_name.is_some() {
                    self.validate_tool(call)
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn validate_tool(&self, call: LlmResponse) -> anyhow::Result<Option<ParsedIntent>> {
        let Some(tool_name) = call.tool_name else {
            return Ok(None);
        };

        if !is_known_tool(&tool_name) {
            tracing::warn!(tool_name = %tool_name, "bedrock returned unknown tool");
            return Ok(None);
        }

        if call.confidence < 0.3 {
            tracing::debug!(confidence = call.confidence, "bedrock confidence too low");
            return Ok(None);
        }

        Ok(Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name,
            tool_args: call.tool_args,
            confidence: call.confidence,
        }))
    }

    fn validate_shell(&self, call: LlmResponse) -> anyhow::Result<Option<ParsedIntent>> {
        let Some(command) = call.command.filter(|c| !c.trim().is_empty()) else {
            return Ok(None);
        };

        if call.confidence < 0.3 {
            return Ok(None);
        }

        Ok(Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: command,
            tool_args: call.tool_args,
            confidence: call.confidence,
        }))
    }

    fn validate_reply(&self, call: LlmResponse) -> anyhow::Result<Option<ParsedIntent>> {
        let Some(message) = call.message.filter(|m| !m.trim().is_empty()) else {
            return Ok(None);
        };

        Ok(Some(ParsedIntent {
            action: ActionKind::Reply,
            tool_name: String::new(),
            tool_args: serde_json::json!({ "message": message }),
            confidence: 1.0,
        }))
    }
}

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

/// Check if a tool name is one of our 10 known tools.
fn is_known_tool(name: &str) -> bool {
    KNOWN_TOOLS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_json ─────────────────────────────────────────────

    #[test]
    fn extract_json_raw() {
        let input = r#"{"tool_name": "read_dtcs", "tool_args": {}, "confidence": 0.9}"#;
        assert_eq!(extract_json(input), input);
    }

    #[test]
    fn extract_json_markdown_json_block() {
        let input = "```json\n{\"tool_name\": \"read_vin\"}\n```";
        assert_eq!(extract_json(input), "{\"tool_name\": \"read_vin\"}");
    }

    #[test]
    fn extract_json_markdown_plain_block() {
        let input = "```\n{\"tool_name\": \"read_vin\"}\n```";
        assert_eq!(extract_json(input), "{\"tool_name\": \"read_vin\"}");
    }

    #[test]
    fn extract_json_with_surrounding_text() {
        let input = "Here is the result:\n```json\n{\"tool_name\": \"log_stats\"}\n```\nDone.";
        assert_eq!(extract_json(input), "{\"tool_name\": \"log_stats\"}");
    }

    // ── is_known_tool ────────────────────────────────────────────

    #[test]
    fn known_tools_accepted() {
        for tool in KNOWN_TOOLS {
            assert!(is_known_tool(tool), "should accept {tool}");
        }
    }

    #[test]
    fn unknown_tool_rejected() {
        assert!(!is_known_tool("hack_ecu"));
        assert!(!is_known_tool(""));
        assert!(!is_known_tool("READ_DTCS")); // case-sensitive
    }

    // ── LlmResponse deserialization ──────────────────────────────

    #[test]
    fn deserialize_tool_action() {
        let json = r#"{"action": "tool", "tool_name": "read_pid", "tool_args": {"pid": "0x0C"}, "confidence": 0.92}"#;
        let resp: LlmResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.action, "tool");
        assert_eq!(resp.tool_name.as_deref(), Some("read_pid"));
        assert_eq!(resp.tool_args["pid"], "0x0C");
    }

    #[test]
    fn deserialize_shell_action() {
        let json = r#"{"action": "shell", "command": "df -h", "confidence": 0.95}"#;
        let resp: LlmResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.action, "shell");
        assert_eq!(resp.command.as_deref(), Some("df -h"));
    }

    #[test]
    fn deserialize_reply_action() {
        let json = r#"{"action": "reply", "message": "Hello!", "confidence": 1.0}"#;
        let resp: LlmResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.action, "reply");
        assert_eq!(resp.message.as_deref(), Some("Hello!"));
    }

    #[test]
    fn deserialize_null_tool_name() {
        let json = r#"{"tool_name": null, "tool_args": {}, "confidence": 0.0}"#;
        let resp: LlmResponse = serde_json::from_str(json).unwrap();
        assert!(resp.tool_name.is_none());
        assert_eq!(resp.action, "tool"); // default
    }

    #[test]
    fn deserialize_missing_optional_fields() {
        let json = r#"{"tool_name": "read_vin"}"#;
        let resp: LlmResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.tool_name.as_deref(), Some("read_vin"));
        assert_eq!(resp.confidence, 0.0);
        assert_eq!(resp.action, "tool");
    }
}
