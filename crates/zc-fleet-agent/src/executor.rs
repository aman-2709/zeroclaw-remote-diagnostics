//! Command executor — dispatches command envelopes to the right action.
//!
//! Bridges between the MQTT command protocol (CommandEnvelope) and:
//! - Tool registry (CAN bus + log tools) for `ActionKind::Tool`
//! - Shell executor for `ActionKind::Shell`
//! - Direct reply for `ActionKind::Reply`
//!
//! Includes a recovery loop: when a tool/shell fails, the executor checks
//! rule-based mappings and optionally Ollama for an alternative intent,
//! then retries once (max 2 attempts total).

use chrono::Utc;
use std::time::{Duration, Instant};

use zc_canbus_tools::CanInterface;
use zc_log_tools::LogSource;
use zc_protocol::commands::{
    ActionKind, AttemptSummary, CommandEnvelope, CommandResponse, CommandStatus, InferenceTier,
    ParsedIntent, RecoverySource,
};

use crate::inference::{EdgeInferenceEngine, sanitize_shell_command};
use crate::recovery::{self, RecoveryContext};
use crate::registry::{ToolKind, ToolRegistry};
use crate::shell;

/// Executes commands by dispatching to the appropriate action handler.
///
/// Generic over CAN interface and log source for testability.
/// Inference is handled by a chain of `EdgeInferenceEngine` implementations;
/// the first engine to return `Some` wins.
pub struct CommandExecutor<'a> {
    registry: &'a ToolRegistry,
    can_interface: &'a dyn CanInterface,
    log_source: &'a dyn LogSource,
    engines: &'a [&'a dyn EdgeInferenceEngine],
}

impl<'a> CommandExecutor<'a> {
    pub fn new(
        registry: &'a ToolRegistry,
        can_interface: &'a dyn CanInterface,
        log_source: &'a dyn LogSource,
        engines: &'a [&'a dyn EdgeInferenceEngine],
    ) -> Self {
        Self {
            registry,
            can_interface,
            log_source,
            engines,
        }
    }

    /// Execute a command envelope and produce a response.
    ///
    /// If `parsed_intent` is present (cloud pre-parsed), uses it directly.
    /// Otherwise attempts local inference via Ollama, falling back to an
    /// error if no match is found.
    ///
    /// On failure, the recovery loop checks rule-based mappings and
    /// optionally Ollama for an alternative, retrying once (max 2 attempts).
    pub async fn execute(&self, envelope: &CommandEnvelope) -> CommandResponse {
        let start = Instant::now();

        // Fast path: intent already parsed by cloud
        let (intent, tier) = if let Some(ref intent) = envelope.parsed_intent {
            (intent.clone(), InferenceTier::Local)
        } else {
            // Try each engine in the chain; first Some wins
            let mut result = None;
            for engine in self.engines {
                if let Some(parsed) = engine.parse(&envelope.natural_language).await {
                    tracing::info!(
                        engine = engine.engine_name(),
                        action = ?parsed.action,
                        tool = %parsed.tool_name,
                        confidence = parsed.confidence,
                        "engine parsed command locally"
                    );
                    result = Some((parsed, InferenceTier::Local));
                    break;
                }
            }
            match result {
                Some(r) => r,
                None if self.engines.is_empty() => {
                    return self.error_response(
                        envelope,
                        start,
                        "no parsed_intent and no inference engines available",
                    );
                }
                None => {
                    return self.error_response(
                        envelope,
                        start,
                        "no inference engine could parse the command",
                    );
                }
            }
        };

        // ── Attempt 1 ───────────────────────────────────────────
        let attempt1_start = Instant::now();
        let response = self.execute_action(envelope, &intent, tier, start).await;
        let attempt1_ms = attempt1_start.elapsed().as_millis() as u64;

        let attempt1 = AttemptSummary {
            attempt: 1,
            action: intent.action,
            tool_name: intent.tool_name.clone(),
            tool_args: intent.tool_args.clone(),
            success: response.status != CommandStatus::Failed,
            error: response.error.clone(),
            duration_ms: attempt1_ms,
            recovery_source: None,
        };

        // Success? Return without attempts metadata.
        if response.status != CommandStatus::Failed {
            return response;
        }

        // Non-recoverable error? Return with single attempt recorded.
        let error_msg = response.error.as_deref().unwrap_or("");
        if !recovery::is_recoverable(error_msg) {
            tracing::debug!(
                error = error_msg,
                "failure is not recoverable — skipping retry"
            );
            return self.with_attempts(response, vec![attempt1]);
        }

        // Check timeout budget (15s total from start of attempt 1)
        let budget = Duration::from_secs(15);
        let remaining = budget.saturating_sub(attempt1_start.elapsed());
        if remaining < Duration::from_secs(2) {
            tracing::debug!("timeout budget exhausted — skipping recovery");
            return self.with_attempts(response, vec![attempt1]);
        }

        // ── Recovery ────────────────────────────────────────────
        let ctx = RecoveryContext {
            original_query: envelope.natural_language.clone(),
            failed_action: intent.action,
            failed_tool: intent.tool_name.clone(),
            failed_args: intent.tool_args.clone(),
            error_message: error_msg.to_string(),
            available_tools: self.registry.tool_names(),
            elapsed_ms: attempt1_ms,
        };

        // Rule-based recovery first (always, free, <1ms)
        let (recovery_intent, source) = if let Some(ri) = recovery::rule_based_recovery(&ctx) {
            tracing::info!(
                from = %intent.tool_name,
                to = %ri.tool_name,
                "rule-based recovery"
            );
            (ri, RecoverySource::RuleBased)
        } else {
            // Try each engine's suggest_recovery in order
            let timeout = remaining.saturating_sub(Duration::from_secs(1));
            let mut found = None;
            for engine in self.engines {
                if let Some(ri) = engine.suggest_recovery(&ctx, timeout).await {
                    tracing::info!(
                        engine = engine.engine_name(),
                        from = %intent.tool_name,
                        to = %ri.tool_name,
                        "engine recovery"
                    );
                    found = Some((ri, engine.recovery_source()));
                    break;
                }
            }
            match found {
                Some(pair) => pair,
                None => return self.with_attempts(response, vec![attempt1]),
            }
        };

        // ── Attempt 2 ───────────────────────────────────────────
        let attempt2_start = Instant::now();
        let response2 = self
            .execute_action(envelope, &recovery_intent, tier, start)
            .await;
        let attempt2_ms = attempt2_start.elapsed().as_millis() as u64;

        let attempt2 = AttemptSummary {
            attempt: 2,
            action: recovery_intent.action,
            tool_name: recovery_intent.tool_name.clone(),
            tool_args: recovery_intent.tool_args.clone(),
            success: response2.status != CommandStatus::Failed,
            error: response2.error.clone(),
            duration_ms: attempt2_ms,
            recovery_source: Some(source),
        };

        self.with_attempts(response2, vec![attempt1, attempt2])
    }

    /// Dispatch a single action (tool, shell, or reply) without recovery.
    async fn execute_action(
        &self,
        envelope: &CommandEnvelope,
        intent: &ParsedIntent,
        tier: InferenceTier,
        start: Instant,
    ) -> CommandResponse {
        match intent.action {
            ActionKind::Tool => self.execute_tool(envelope, intent, tier, start).await,
            ActionKind::Shell => self.execute_shell(envelope, intent, tier, start).await,
            ActionKind::Reply => self.execute_reply(envelope, intent, tier, start),
        }
    }

    /// Attach attempt summaries to a response. Only sets the field when
    /// there are multiple attempts (single-attempt successes stay clean).
    fn with_attempts(
        &self,
        mut resp: CommandResponse,
        attempts: Vec<AttemptSummary>,
    ) -> CommandResponse {
        resp.attempts = if attempts.len() > 1 {
            Some(attempts)
        } else {
            None
        };
        resp
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
                // ToolResult has a `success` field — respect it.
                // Tools like read_uds_did return Ok(ToolResult { success: false, error: "..." })
                // for soft failures (unknown DID, all reads failed, etc.).
                let tool_success = data["success"].as_bool().unwrap_or(true);

                if tool_success {
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
                        attempts: None,
                    }
                } else {
                    let error_msg = data["error"]
                        .as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("Tool '{tool_name}' reported failure"));
                    CommandResponse {
                        command_id: envelope.id,
                        correlation_id: envelope.correlation_id,
                        device_id: envelope.device_id.clone(),
                        status: CommandStatus::Failed,
                        inference_tier: tier,
                        response_text: None,
                        response_data: Some(data),
                        latency_ms,
                        responded_at: Utc::now(),
                        error: Some(error_msg),
                        attempts: None,
                    }
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
                attempts: None,
            },
        }
    }

    /// Execute a shell action via the safe shell executor.
    ///
    /// Sanitizes commands before execution as defense-in-depth — cloud inference
    /// may generate piped commands that the shell executor would reject.
    async fn execute_shell(
        &self,
        envelope: &CommandEnvelope,
        intent: &ParsedIntent,
        tier: InferenceTier,
        start: Instant,
    ) -> CommandResponse {
        // Sanitize: strip everything from first shell metacharacter onward.
        // Cloud/Bedrock inference may produce piped commands; we only run the base command.
        let command_str = sanitize_shell_command(&intent.tool_name);
        if command_str.is_empty() {
            let latency_ms = start.elapsed().as_millis() as u64;
            return CommandResponse {
                command_id: envelope.id,
                correlation_id: envelope.correlation_id,
                device_id: envelope.device_id.clone(),
                status: CommandStatus::Failed,
                inference_tier: tier,
                response_text: None,
                response_data: None,
                latency_ms,
                responded_at: Utc::now(),
                error: Some("shell: command was empty after sanitization".into()),
                attempts: None,
            };
        }
        if command_str != intent.tool_name {
            tracing::info!(
                original = %intent.tool_name,
                sanitized = %command_str,
                "executor sanitized shell command from cloud intent"
            );
        }

        match shell::execute(&command_str).await {
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

                // Provide a helpful message when the command succeeds but produces no output
                if output.trim().is_empty() {
                    output = format!("(no output from `{command_str}`)");
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
                    attempts: None,
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
                    attempts: None,
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
            attempts: None,
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
            attempts: None,
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

    use crate::inference::{EdgeInferenceEngine, FallbackReplyEngine, OllamaClient, OllamaConfig};

    /// Helper: build executor with no inference engines (pre-parsed intent only).
    fn make_executor<'a>(
        registry: &'a ToolRegistry,
        can: &'a MockCanInterface,
        logs: &'a MockLogSource,
    ) -> CommandExecutor<'a> {
        CommandExecutor::new(registry, can, logs, &[])
    }

    // ── Tool action tests (existing) ─────────────────────────────

    #[tokio::test]
    async fn execute_without_intent_no_engines_fails() {
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
                .contains("no inference engines available")
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

    #[tokio::test]
    async fn execute_shell_empty_output_shows_message() {
        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let executor = make_executor(&registry, &can, &logs);

        // `ip -details link show type can` returns empty on machines without CAN interfaces
        let mut cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "show CAN interface", "admin");
        cmd.parsed_intent = Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "ip -details link show type can".into(),
            tool_args: json!({}),
            confidence: 0.85,
        });
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Completed);
        let text = resp.response_text.unwrap();
        assert!(
            text.contains("no output"),
            "empty shell output should show helpful message, got: {text}"
        );
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
        let engines: Vec<&dyn EdgeInferenceEngine> = vec![&ollama];
        let executor = CommandExecutor::new(&registry, &can, &logs, &engines);

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
        let engines: Vec<&dyn EdgeInferenceEngine> = vec![&ollama];
        let executor = CommandExecutor::new(&registry, &can, &logs, &engines);

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
        let engines: Vec<&dyn EdgeInferenceEngine> = vec![&ollama];
        let executor = CommandExecutor::new(&registry, &can, &logs, &engines);

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
        let engines: Vec<&dyn EdgeInferenceEngine> = vec![&ollama];
        let executor = CommandExecutor::new(&registry, &can, &logs, &engines);

        let cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "bake a pizza", "admin");
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Failed);
        assert!(resp.error.unwrap().contains("no inference engine"));
    }

    // ── Recovery loop tests ─────────────────────────────────────

    /// Rule-based recovery: search_logs file-not-found → query_journal.
    /// Verifies the full executor recovery path, not just the rule module.
    #[tokio::test]
    async fn recovery_rule_based_log_tool_fallback() {
        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        // Use default MockLogSource which does NOT have /var/log/nonexistent
        let logs = MockLogSource::with_syslog_sample();
        let executor = make_executor(&registry, &can, &logs);

        let mut cmd =
            CommandEnvelope::new("fleet-alpha", "rpi-001", "search logs for errors", "admin");
        cmd.parsed_intent = Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "search_logs".into(),
            tool_args: json!({"path": "/var/log/nonexistent", "query": "error"}),
            confidence: 0.9,
        });
        let resp = executor.execute(&cmd).await;

        // The mock returns "not found" for unknown paths → triggers recovery
        // to query_journal, which also fails (no journalctl in tests).
        // Key assertion: attempt chain is recorded with 2 entries.
        let attempts = resp.attempts.expect("should have attempt chain");
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].tool_name, "search_logs");
        assert!(!attempts[0].success);
        assert!(attempts[0].recovery_source.is_none());
        assert_eq!(attempts[1].tool_name, "query_journal");
        assert_eq!(attempts[1].recovery_source, Some(RecoverySource::RuleBased));
    }

    /// Both attempts fail → response should still carry the full attempt chain.
    #[tokio::test]
    async fn recovery_both_attempts_fail() {
        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let executor = make_executor(&registry, &can, &logs);

        // read_dtcs with mock CAN (no queued frames) → timeout/error → recovery
        // to read_uds_dtcs(BCR) which also fails with mock CAN.
        let mut cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "read DTCs", "admin");
        cmd.parsed_intent = Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "read_dtcs".into(),
            tool_args: json!({}),
            confidence: 0.95,
        });
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Failed);
        let attempts = resp.attempts.expect("should have attempt chain");
        assert_eq!(attempts.len(), 2);
        assert!(!attempts[0].success, "attempt 1 should fail");
        assert_eq!(attempts[0].tool_name, "read_dtcs");
        assert!(!attempts[1].success, "attempt 2 should also fail");
        assert_eq!(attempts[1].tool_name, "read_uds_dtcs");
        assert_eq!(attempts[1].recovery_source, Some(RecoverySource::RuleBased));
    }

    /// Non-recoverable error (blocked command) should NOT trigger recovery.
    #[tokio::test]
    async fn recovery_skipped_for_non_recoverable_error() {
        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let executor = make_executor(&registry, &can, &logs);

        let mut cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "delete files", "admin");
        cmd.parsed_intent = Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "rm -rf /".into(),
            tool_args: json!({}),
            confidence: 0.9,
        });
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Failed);
        // "blocked" is non-recoverable → no recovery attempted → no attempts chain
        assert!(
            resp.attempts.is_none(),
            "non-recoverable error should not produce attempt chain"
        );
    }

    /// Engine recovery path: tool fails, no rule matches, engine suggests
    /// an alternative that is then executed.
    #[tokio::test]
    async fn recovery_engine_fallback_suggests_alternative() {
        let server = MockServer::start().await;

        // Recovery call: Ollama suggests log_stats as fallback for read_pid.
        let recovery_body = ollama_response(
            r#"{"action": "tool", "tool_name": "log_stats", "tool_args": {"path": "/var/log/syslog"}, "confidence": 0.8}"#,
        );

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&recovery_body))
            .expect(1)
            .mount(&server)
            .await;

        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let ollama = ollama_client_for(&server);
        let engines: Vec<&dyn EdgeInferenceEngine> = vec![&ollama];
        let executor = CommandExecutor::new(&registry, &can, &logs, &engines);

        // read_pid fails with Err(Timeout) on mock CAN (no queued responses),
        // and has no rule-based recovery mapping → falls through to engine chain.
        let mut cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "read engine RPM", "admin");
        cmd.parsed_intent = Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "read_pid".into(),
            tool_args: json!({"pid": "0x0C"}),
            confidence: 0.9,
        });
        let resp = executor.execute(&cmd).await;

        // log_stats should succeed on the mock log source
        assert_eq!(resp.status, CommandStatus::Completed);
        let attempts = resp.attempts.expect("should have attempt chain");
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].tool_name, "read_pid");
        assert!(!attempts[0].success);
        assert_eq!(attempts[1].tool_name, "log_stats");
        assert!(attempts[1].success);
        assert_eq!(attempts[1].recovery_source, Some(RecoverySource::Ollama));
    }

    /// Engine recovery returns same tool that failed → rejected, no retry.
    #[tokio::test]
    async fn recovery_engine_rejects_same_tool() {
        let server = MockServer::start().await;

        // Recovery suggests the SAME tool that failed → should be rejected.
        let recovery_body = ollama_response(
            r#"{"action": "tool", "tool_name": "read_pid", "tool_args": {"pid": "0x0C"}, "confidence": 0.85}"#,
        );

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&recovery_body))
            .expect(1)
            .mount(&server)
            .await;

        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let ollama = ollama_client_for(&server);
        let engines: Vec<&dyn EdgeInferenceEngine> = vec![&ollama];
        let executor = CommandExecutor::new(&registry, &can, &logs, &engines);

        // read_pid fails on mock CAN, no rule-based recovery, engine
        // suggests same tool → rejected → single attempt, no chain.
        let mut cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "read engine RPM", "admin");
        cmd.parsed_intent = Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "read_pid".into(),
            tool_args: json!({"pid": "0x0C"}),
            confidence: 0.9,
        });
        let resp = executor.execute(&cmd).await;

        // Same-tool suggestion rejected → no second attempt → no attempts chain
        assert_eq!(resp.status, CommandStatus::Failed);
        assert!(
            resp.attempts.is_none(),
            "same-tool rejection should result in no attempt chain (single attempt)"
        );
    }

    /// Success on first attempt → no attempt chain in response.
    #[tokio::test]
    async fn success_first_attempt_no_attempts_chain() {
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
        assert!(
            resp.attempts.is_none(),
            "successful first attempt should not have attempts metadata"
        );
    }

    /// Attempt durations are recorded and non-zero (or at least non-negative).
    #[tokio::test]
    async fn recovery_attempt_durations_recorded() {
        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let executor = make_executor(&registry, &can, &logs);

        let mut cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "read VIN", "admin");
        cmd.parsed_intent = Some(ParsedIntent {
            action: ActionKind::Tool,
            tool_name: "read_vin".into(),
            tool_args: json!({}),
            confidence: 0.95,
        });
        let resp = executor.execute(&cmd).await;

        let attempts = resp.attempts.expect("should have attempt chain");
        assert_eq!(attempts.len(), 2);
        // Durations should be recorded (may be 0 if mock is very fast, but should exist)
        for att in &attempts {
            assert!(att.duration_ms < 15_000, "duration should be within budget");
        }
    }

    // ── Fallback engine chain tests ──────────────────────────────

    /// Fallback-only chain handles greetings without any LLM.
    #[tokio::test]
    async fn fallback_only_chain_handles_greeting() {
        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let fallback = FallbackReplyEngine;
        let engines: Vec<&dyn EdgeInferenceEngine> = vec![&fallback];
        let executor = CommandExecutor::new(&registry, &can, &logs, &engines);

        let cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "hello", "admin");
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Completed);
        assert!(resp.response_text.unwrap().contains("fleet agent"));
    }

    /// Fallback-only chain doesn't match diagnostic queries.
    #[tokio::test]
    async fn fallback_only_chain_no_match_diagnostic() {
        let registry = ToolRegistry::with_defaults();
        let can = MockCanInterface::new();
        let logs = MockLogSource::with_syslog_sample();
        let fallback = FallbackReplyEngine;
        let engines: Vec<&dyn EdgeInferenceEngine> = vec![&fallback];
        let executor = CommandExecutor::new(&registry, &can, &logs, &engines);

        let cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "read DTCs", "admin");
        let resp = executor.execute(&cmd).await;

        assert_eq!(resp.status, CommandStatus::Failed);
        assert!(resp.error.unwrap().contains("no inference engine"));
    }

    /// Ollama + Fallback chain: Ollama handles diagnostics, fallback catches greetings.
    #[tokio::test]
    async fn ollama_plus_fallback_chain_greeting_handled() {
        // Ollama returns no match for "hello" (returns null tool_name)
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
        let fallback = FallbackReplyEngine;
        let engines: Vec<&dyn EdgeInferenceEngine> = vec![&ollama, &fallback];
        let executor = CommandExecutor::new(&registry, &can, &logs, &engines);

        let cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "hello", "admin");
        let resp = executor.execute(&cmd).await;

        // Fallback catches the greeting even though Ollama didn't match
        assert_eq!(resp.status, CommandStatus::Completed);
        assert!(resp.response_text.unwrap().contains("fleet agent"));
    }
}
