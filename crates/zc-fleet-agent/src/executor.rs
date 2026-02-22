//! Command executor — dispatches command envelopes to the right tool.
//!
//! Bridges between the MQTT command protocol (CommandEnvelope) and the
//! tool registry (CAN bus + log tools).

use chrono::Utc;
use std::time::Instant;

use zc_canbus_tools::CanInterface;
use zc_log_tools::LogSource;
use zc_protocol::commands::{
    CommandEnvelope, CommandResponse, CommandStatus, InferenceTier, ParsedIntent,
};

use crate::inference::OllamaClient;
use crate::registry::{ToolKind, ToolRegistry};

/// Executes commands by dispatching to the appropriate tool.
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
                        "no tool match for command — local inference returned no result",
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

        let intent: &ParsedIntent = &intent;

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
            Ok(data) => CommandResponse {
                command_id: envelope.id,
                correlation_id: envelope.correlation_id,
                device_id: envelope.device_id.clone(),
                status: CommandStatus::Completed,
                inference_tier: tier,
                response_text: Some(format!("Tool '{tool_name}' executed successfully")),
                response_data: Some(data),
                latency_ms,
                responded_at: Utc::now(),
                error: None,
            },
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
            tool_name: "search_logs".into(),
            tool_args: json!({"path": "/var/log/syslog", "query": "error"}),
            confidence: 0.88,
        });
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.command_id, cmd.id);
        assert_eq!(resp.correlation_id, cmd.correlation_id);
        assert_eq!(resp.device_id, "rpi-001");
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
    async fn execute_ollama_inference_succeeds() {
        let server = MockServer::start().await;
        let body = ollama_response(
            r#"{"tool_name": "log_stats", "tool_args": {"path": "/var/log/syslog"}, "confidence": 0.92}"#,
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

        // No parsed_intent — should fall through to Ollama
        let cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "show me log stats", "admin");
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Completed);
        assert!(resp.response_data.is_some());
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
        assert!(resp.error.unwrap().contains("no tool match"));
    }
}
