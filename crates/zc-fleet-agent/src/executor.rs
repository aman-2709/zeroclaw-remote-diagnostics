//! Command executor — dispatches command envelopes to the right action.
//!
//! Bridges between the MQTT command protocol (CommandEnvelope) and:
//! - Tool registry (CAN bus + log tools) for `ActionKind::Tool`
//! - Shell executor for `ActionKind::Shell`
//! - Direct reply for `ActionKind::Reply`

use chrono::Utc;
use std::time::Instant;

use zc_canbus_tools::CanInterface;
use zc_log_tools::LogSource;
use zc_protocol::commands::{
    ActionKind, CommandEnvelope, CommandResponse, CommandStatus, InferenceTier, ParsedIntent,
};

use crate::inference::OllamaClient;
use crate::registry::{ToolKind, ToolRegistry};
use crate::shell;

/// Executes commands by dispatching to the appropriate action handler.
///
/// Generic over CAN interface and log source for testability.
pub struct CommandExecutor<'a> {
    registry: &'a ToolRegistry,
    can_interface: &'a dyn CanInterface,
    log_source: &'a dyn LogSource,
    ollama: Option<&'a OllamaClient>,
}

impl<'a> CommandExecutor<'a> {
    pub fn new(
        registry: &'a ToolRegistry,
        can_interface: &'a dyn CanInterface,
        log_source: &'a dyn LogSource,
        ollama: Option<&'a OllamaClient>,
    ) -> Self {
        Self {
            registry,
            can_interface,
            log_source,
            ollama,
        }
    }

    /// Execute a command envelope and produce a response.
    ///
    /// If `parsed_intent` is present (cloud pre-parsed), uses it directly.
    /// Otherwise attempts local inference via Ollama, falling back to an
    /// error if no match is found.
    pub async fn execute(&self, envelope: &CommandEnvelope) -> CommandResponse {
        let start = Instant::now();

        // Fast path: intent already parsed by cloud
        let (intent, tier) = if let Some(ref intent) = envelope.parsed_intent {
            (intent.clone(), InferenceTier::Local)
        } else if let Some(ollama) = self.ollama {
            // Local inference via Ollama
            match ollama.parse(&envelope.natural_language).await {
                Some(parsed) => {
                    tracing::info!(
                        action = ?parsed.action,
                        tool = %parsed.tool_name,
                        confidence = parsed.confidence,
                        "ollama parsed command locally"
                    );
                    (parsed, InferenceTier::Local)
                }
                None => {
                    return self.error_response(
                        envelope,
                        start,
                        "no match for command — local inference returned no result",
                    );
                }
            }
        } else {
            return self.error_response(
                envelope,
                start,
                "no parsed_intent and local inference not available",
            );
        };

        // Route based on action kind
        match intent.action {
            ActionKind::Tool => self.execute_tool(envelope, &intent, tier, start).await,
            ActionKind::Shell => self.execute_shell(envelope, &intent, tier, start).await,
            ActionKind::Reply => self.execute_reply(envelope, &intent, tier, start),
        }
    }

    /// Execute a tool action via the ToolRegistry.
    async fn execute_tool(
        &self,
        envelope: &CommandEnvelope,
        intent: &ParsedIntent,
        tier: InferenceTier,
        start: Instant,
    ) -> CommandResponse {
        let tool_name = &intent.tool_name;
        let Some((kind, idx)) = self.registry.lookup(tool_name) else {
            return self.error_response(envelope, start, &format!("unknown tool: {tool_name}"));
        };

        let result = match kind {
            ToolKind::CanBus => {
                self.registry
                    .execute_can(idx, intent.tool_args.clone(), self.can_interface)
                    .await
            }
            ToolKind::Log => {
                self.registry
                    .execute_log(idx, intent.tool_args.clone(), self.log_source)
                    .await
            }
        };

        let latency_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(data) => {
                // Prefer the tool's summary (e.g. "Found 5 matches …") over a generic message
                let summary = data["summary"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("Tool '{tool_name}' executed successfully"));
                CommandResponse {
                    command_id: envelope.id,
                    correlation_id: envelope.correlation_id,
                    device_id: envelope.device_id.clone(),
                    status: CommandStatus::Completed,
                    inference_tier: tier,
                    response_text: Some(summary),
                    response_data: Some(data),
                    latency_ms,
                    responded_at: Utc::now(),
                    error: None,
                }
            }
            Err(err) => CommandResponse {
                command_id: envelope.id,
                correlation_id: envelope.correlation_id,
                device_id: envelope.device_id.clone(),
                status: CommandStatus::Failed,
                inference_tier: tier,
                response_text: None,
                response_data: None,
                latency_ms,
                responded_at: Utc::now(),
                error: Some(err),
            },
        }
    }

    /// Execute a shell action via the safe shell executor.
    async fn execute_shell(
        &self,
        envelope: &CommandEnvelope,
        intent: &ParsedIntent,
        tier: InferenceTier,
        start: Instant,
    ) -> CommandResponse {
        let command_str = &intent.tool_name;

        match shell::execute(command_str).await {
            Ok(result) => {
                let mut output = result.stdout;
                if !result.stderr.is_empty() {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str(&format!("[stderr] {}", result.stderr));
                }
                if result.truncated {
                    tracing::info!(command = %command_str, "shell output was truncated");
                }

                let latency_ms = start.elapsed().as_millis() as u64;
                CommandResponse {
                    command_id: envelope.id,
                    correlation_id: envelope.correlation_id,
                    device_id: envelope.device_id.clone(),
                    status: CommandStatus::Completed,
                    inference_tier: tier,
                    response_text: Some(output),
                    response_data: None,
                    latency_ms,
                    responded_at: Utc::now(),
                    error: None,
                }
            }
            Err(e) => {
                let latency_ms = start.elapsed().as_millis() as u64;
                CommandResponse {
                    command_id: envelope.id,
                    correlation_id: envelope.correlation_id,
                    device_id: envelope.device_id.clone(),
                    status: CommandStatus::Failed,
                    inference_tier: tier,
                    response_text: None,
                    response_data: None,
                    latency_ms,
                    responded_at: Utc::now(),
                    error: Some(format!("shell: {e}")),
                }
            }
        }
    }

    /// Execute a reply action — extract message and return as response_text.
    fn execute_reply(
        &self,
        envelope: &CommandEnvelope,
        intent: &ParsedIntent,
        tier: InferenceTier,
        start: Instant,
    ) -> CommandResponse {
        let message = intent
            .tool_args
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("(no response)")
            .to_string();

        CommandResponse {
            command_id: envelope.id,
            correlation_id: envelope.correlation_id,
            device_id: envelope.device_id.clone(),
            status: CommandStatus::Completed,
            inference_tier: tier,
            response_text: Some(message),
            response_data: None,
            latency_ms: start.elapsed().as_millis() as u64,
            responded_at: Utc::now(),
            error: None,
        }
    }

    fn error_response(
        &self,
        envelope: &CommandEnvelope,
        start: Instant,
        message: &str,
    ) -> CommandResponse {
        CommandResponse {
            command_id: envelope.id,
            correlation_id: envelope.correlation_id,
            device_id: envelope.device_id.clone(),
            status: CommandStatus::Failed,
            inference_tier: InferenceTier::Local,
            response_text: None,
            response_data: None,
            latency_ms: start.elapsed().as_millis() as u64,
            responded_at: Utc::now(),
            error: Some(message.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use zc_canbus_tools::MockCanInterface;
    use zc_log_tools::MockLogSource;
    use zc_protocol::commands::ParsedIntent;

    use crate::inference::OllamaConfig;

    /// Helper: build executor without Ollama (backward-compat path).
    fn make_executor<'a>(
        registry: &'a ToolRegistry,
        can: &'a MockCanInterface,
        logs: &'a MockLogSource,
    ) -> CommandExecutor<'a> {
        CommandExecutor::new(registry, can, logs, None)
    }

    // ── Tool action tests (existing) ─────────────────────────────

    #[tokio::test]
    async fn execute_without_intent_no_ollama_fails() {
        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let executor = make_executor(&registry, &can, &logs);

        let cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "read DTCs", "admin");
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Failed);
        assert!(
            resp.error
                .unwrap()
                .contains("local inference not available")
        );
    }

    #[tokio::test]
    async fn execute_unknown_tool_fails() {
        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let executor = make_executor(&registry, &can, &logs);

        let mut cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "do magic", "admin");
        cmd.parsed_intent = Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "nonexistent_tool".into(),
            tool_args: json!({}),
            confidence: 0.9,
        });
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Failed);
        assert!(resp.error.unwrap().contains("unknown tool"));
    }

    #[tokio::test]
    async fn execute_log_tool_succeeds() {
        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let executor = make_executor(&registry, &can, &logs);

        let mut cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "show log stats", "admin");
        cmd.parsed_intent = Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "log_stats".into(),
            tool_args: json!({"path": "/var/log/syslog"}),
            confidence: 0.95,
        });
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Completed);
        assert!(resp.response_data.is_some());
        assert!(resp.latency_ms < 1000);
    }

    #[tokio::test]
    async fn execute_preserves_ids() {
        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let executor = make_executor(&registry, &can, &logs);

        let mut cmd =
            CommandEnvelope::new("fleet-alpha", "rpi-001", "search logs for error", "admin");
        cmd.parsed_intent = Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "search_logs".into(),
            tool_args: json!({"path": "/var/log/syslog", "query": "error"}),
            confidence: 0.88,
        });
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.command_id, cmd.id);
        assert_eq!(resp.correlation_id, cmd.correlation_id);
        assert_eq!(resp.device_id, "rpi-001");
    }

    // ── Shell action tests ───────────────────────────────────────

    #[tokio::test]
    async fn execute_shell_command() {
        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let executor = make_executor(&registry, &can, &logs);

        let mut cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "show hostname", "admin");
        cmd.parsed_intent = Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "hostname".into(),
            tool_args: json!({}),
            confidence: 0.9,
        });
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Completed);
        assert!(resp.response_text.is_some());
        assert!(!resp.response_text.unwrap().is_empty());
    }

    #[tokio::test]
    async fn execute_shell_blocked_command_fails() {
        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let executor = make_executor(&registry, &can, &logs);

        let mut cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "delete all", "admin");
        cmd.parsed_intent = Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "rm -rf /".into(),
            tool_args: json!({}),
            confidence: 0.9,
        });
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Failed);
        assert!(resp.error.unwrap().contains("shell:"));
    }

    // ── Reply action tests ───────────────────────────────────────

    #[tokio::test]
    async fn execute_reply_returns_message() {
        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let executor = make_executor(&registry, &can, &logs);

        let mut cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "how are you?", "admin");
        cmd.parsed_intent = Some(ParsedIntent {
            action: ActionKind::Reply,
            tool_name: String::new(),
            tool_args: json!({"message": "I'm operational and monitoring the fleet."}),
            confidence: 1.0,
        });
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Completed);
        assert_eq!(
            resp.response_text.unwrap(),
            "I'm operational and monitoring the fleet."
        );
        assert!(resp.response_data.is_none());
    }

    #[tokio::test]
    async fn execute_reply_missing_message() {
        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let executor = make_executor(&registry, &can, &logs);

        let mut cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "...", "admin");
        cmd.parsed_intent = Some(ParsedIntent {
            action: ActionKind::Reply,
            tool_name: String::new(),
            tool_args: json!({}),
            confidence: 1.0,
        });
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Completed);
        assert_eq!(resp.response_text.unwrap(), "(no response)");
    }

    // ── Ollama inference path tests ──────────────────────────────

    fn ollama_response(content: &str) -> serde_json::Value {
        json!({
            "model": "phi3:mini",
            "message": { "role": "assistant", "content": content },
            "done": true
        })
    }

    fn ollama_client_for(server: &MockServer) -> OllamaClient {
        OllamaClient::new(OllamaConfig {
            host: server.uri(),
            model: "phi3:mini".into(),
            timeout_secs: 2,
            enabled: true,
        })
    }

    #[tokio::test]
    async fn execute_ollama_tool_inference_succeeds() {
        let server = MockServer::start().await;
        let body = ollama_response(
            r#"{"action": "tool", "tool_name": "log_stats", "tool_args": {"path": "/var/log/syslog"}, "confidence": 0.92}"#,
        );
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let ollama = ollama_client_for(&server);
        let executor = CommandExecutor::new(&registry, &can, &logs, Some(&ollama));

        let cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "show me log stats", "admin");
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Completed);
        assert!(resp.response_data.is_some());
    }

    #[tokio::test]
    async fn execute_ollama_shell_inference() {
        let server = MockServer::start().await;
        let body =
            ollama_response(r#"{"action": "shell", "command": "hostname", "confidence": 0.9}"#);
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let ollama = ollama_client_for(&server);
        let executor = CommandExecutor::new(&registry, &can, &logs, Some(&ollama));

        let cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "what's the hostname?", "admin");
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Completed);
        assert!(resp.response_text.is_some());
    }

    #[tokio::test]
    async fn execute_ollama_reply_inference() {
        let server = MockServer::start().await;
        let body = ollama_response(
            r#"{"action": "reply", "message": "Hello! How can I help?", "confidence": 1.0}"#,
        );
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let ollama = ollama_client_for(&server);
        let executor = CommandExecutor::new(&registry, &can, &logs, Some(&ollama));

        let cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "hello", "admin");
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Completed);
        assert_eq!(resp.response_text.unwrap(), "Hello! How can I help?");
    }

    #[tokio::test]
    async fn execute_ollama_no_match_fails() {
        let server = MockServer::start().await;
        let body = ollama_response(r#"{"tool_name": null, "tool_args": {}, "confidence": 0.0}"#);
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .mount(&server)
            .await;

        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let ollama = ollama_client_for(&server);
        let executor = CommandExecutor::new(&registry, &can, &logs, Some(&ollama));

        let cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "bake a pizza", "admin");
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Failed);
        assert!(resp.error.unwrap().contains("no match"));
    }
}
