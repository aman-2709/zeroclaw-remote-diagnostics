//! Recovery logic for failed tool/shell executions.
//!
//! When a command fails, the recovery module determines whether the failure
//! is recoverable and suggests an alternative intent. Two tiers:
//! 1. **Rule-based** — deterministic mappings (free, <1ms)
//! 2. **Ollama** — local LLM suggestion (when no rule matches)

use std::time::Duration;

use serde_json::json;
use zc_protocol::commands::{ActionKind, ParsedIntent};

use crate::inference::OllamaClient;

/// Context about a failed execution attempt, used to determine recovery.
#[derive(Debug)]
pub struct RecoveryContext {
    /// Original natural-language query from the operator.
    pub original_query: String,
    /// What kind of action failed.
    pub failed_action: ActionKind,
    /// Tool name or shell command that failed.
    pub failed_tool: String,
    /// Arguments passed to the failed tool.
    pub failed_args: serde_json::Value,
    /// Error message from the failure.
    pub error_message: String,
    /// Names of all available tools (for Ollama prompt).
    pub available_tools: Vec<String>,
    /// Elapsed time so far in milliseconds.
    pub elapsed_ms: u64,
}

/// Returns `false` for errors that should never be retried (safety blocks,
/// injection detection, sensitive paths).
pub fn is_recoverable(error: &str) -> bool {
    let lower = error.to_lowercase();
    // Safety violations are intentional blocks — never retry
    if lower.contains("safety violation") {
        return false;
    }
    // Shell command was blocked by allowlist
    if lower.contains("blocked") {
        return false;
    }
    // Injection/metacharacter detection
    if lower.contains("injection") {
        return false;
    }
    // Restricted file path access
    if lower.contains("sensitive path") {
        return false;
    }
    true
}

/// Try deterministic rule-based recovery. Returns `Some(ParsedIntent)` if a
/// known alternative exists for the failed tool + error combination.
pub fn rule_based_recovery(ctx: &RecoveryContext) -> Option<ParsedIntent> {
    match ctx.failed_action {
        ActionKind::Tool => rule_based_tool_recovery(ctx),
        ActionKind::Shell => rule_based_shell_recovery(ctx),
        ActionKind::Reply => None, // replies don't fail in a recoverable way
    }
}

/// Rule-based recovery for failed tool actions.
fn rule_based_tool_recovery(ctx: &RecoveryContext) -> Option<ParsedIntent> {
    let error = &ctx.error_message;
    let tool = ctx.failed_tool.as_str();

    // Log tools: syslog not found → try journald, and vice versa
    let is_file_error = error.contains("not found")
        || error.contains("No such file")
        || error.contains("does not exist");

    match tool {
        "search_logs" | "tail_logs" | "log_stats" | "analyze_errors" if is_file_error => {
            // Extract query from original args for search_logs
            let query = ctx
                .failed_args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let unit = ctx
                .failed_args
                .get("unit")
                .and_then(|v| v.as_str())
                .unwrap_or("syslog");

            let mut args = json!({"unit": unit, "lines": 50});
            if !query.is_empty() {
                // query_journal doesn't have a query param, but we pass it
                // so the tool can filter if supported
                args["query"] = json!(query);
            }

            Some(ParsedIntent {
                action: ActionKind::Tool,
                tool_name: "query_journal".into(),
                tool_args: args,
                confidence: 0.85,
            })
        }

        "query_journal" if is_file_error || error.contains("exec") => {
            // journald not available → fall back to syslog file
            Some(ParsedIntent {
                action: ActionKind::Tool,
                tool_name: "search_logs".into(),
                tool_args: json!({"path": "/var/log/syslog", "query": ""}),
                confidence: 0.85,
            })
        }

        // OBD-II read_dtcs timeout → try UDS read_uds_dtcs on BCR
        "read_dtcs" if error.contains("timeout") || error.contains("Timeout") => {
            Some(ParsedIntent {
                action: ActionKind::Tool,
                tool_name: "read_uds_dtcs".into(),
                tool_args: json!({"ecu": "BCR"}),
                confidence: 0.80,
            })
        }

        // UDS BCR timeout → try BCF
        "read_uds_dtcs"
            if (error.contains("timeout") || error.contains("Timeout"))
                && ctx
                    .failed_args
                    .get("ecu")
                    .and_then(|v| v.as_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("BCR")) =>
        {
            Some(ParsedIntent {
                action: ActionKind::Tool,
                tool_name: "read_uds_dtcs".into(),
                tool_args: json!({"ecu": "BCF"}),
                confidence: 0.75,
            })
        }

        // OBD-II read_vin timeout → try UDS DID 0xF190 (VIN) on BCR
        "read_vin" if error.contains("timeout") || error.contains("Timeout") => {
            Some(ParsedIntent {
                action: ActionKind::Tool,
                tool_name: "read_uds_did".into(),
                tool_args: json!({"ecu": "BCR", "did": 0xF190_u16}),
                confidence: 0.80,
            })
        }

        _ => None,
    }
}

/// Rule-based recovery for failed shell actions.
fn rule_based_shell_recovery(ctx: &RecoveryContext) -> Option<ParsedIntent> {
    let error = &ctx.error_message;
    let cmd = ctx.failed_tool.as_str();

    let is_not_found = error.contains("not found") || error.contains("No such file");

    // `sensors` not found → read thermal_zone directly
    if cmd == "sensors" && is_not_found {
        return Some(ParsedIntent {
            action: ActionKind::Shell,
            tool_name: "cat /sys/class/thermal/thermal_zone0/temp".into(),
            tool_args: json!({}),
            confidence: 0.80,
        });
    }

    None
}

/// System prompt template for Ollama recovery suggestions.
const RECOVERY_PROMPT_TEMPLATE: &str = r#"A diagnostic command failed on an IoT edge device. Suggest an alternative.

Original query: "{query}"
Attempted: {tool_name}({args})
Error: {error}
Available tools: [{tool_list}]

Rules:
- Do NOT suggest "{failed_tool}" again
- Respond with ONLY a JSON object
- For a tool: {{"action": "tool", "tool_name": "...", "tool_args": {{}}, "confidence": 0.8}}
- For a shell command: {{"action": "shell", "command": "...", "confidence": 0.8}}
- If no good alternative exists, respond: {{"action": "reply", "message": "No alternative available", "confidence": 1.0}}"#;

/// Build the Ollama recovery prompt from context.
fn build_recovery_prompt(ctx: &RecoveryContext) -> (String, String) {
    let system = RECOVERY_PROMPT_TEMPLATE
        .replace("{query}", &ctx.original_query)
        .replace("{tool_name}", &ctx.failed_tool)
        .replace("{args}", &ctx.failed_args.to_string())
        .replace("{error}", &ctx.error_message)
        .replace("{tool_list}", &ctx.available_tools.join(", "))
        .replace("{failed_tool}", &ctx.failed_tool);

    let user_msg = format!(
        "The command `{}` failed with: {}. What should I try instead?",
        ctx.failed_tool, ctx.error_message
    );

    (system, user_msg)
}

/// Ask Ollama for a recovery suggestion. Returns `None` on timeout, parse
/// failure, or if the suggestion is the same tool that already failed.
pub async fn ollama_recovery(
    ctx: &RecoveryContext,
    ollama: &OllamaClient,
    timeout: Duration,
) -> Option<ParsedIntent> {
    let (system_prompt, user_msg) = build_recovery_prompt(ctx);

    let result = tokio::time::timeout(timeout, ollama.parse_with_prompt(&system_prompt, &user_msg))
        .await
        .ok()
        .flatten()?;

    // Reject if the suggestion is the same tool that failed
    if result.action == ctx.failed_action && result.tool_name == ctx.failed_tool {
        tracing::info!(
            tool = %ctx.failed_tool,
            "ollama recovery suggested same tool — rejecting"
        );
        return None;
    }

    // Reject reply suggestions (means Ollama couldn't find an alternative)
    if result.action == ActionKind::Reply {
        tracing::info!("ollama recovery returned reply — no alternative found");
        return None;
    }

    Some(result)
}

/// Ask the Bedrock edge engine for a recovery suggestion. Mirrors `ollama_recovery()`
/// but uses the Bedrock Converse API. Returns `None` on timeout, parse failure,
/// or if the suggestion is the same tool that already failed.
#[cfg(feature = "bedrock")]
pub async fn bedrock_recovery(
    ctx: &RecoveryContext,
    engine: &crate::bedrock::EdgeBedrockEngine,
    timeout: Duration,
) -> Option<ParsedIntent> {
    let (system_prompt, user_msg) = build_recovery_prompt(ctx);

    let result = tokio::time::timeout(
        timeout,
        engine.call_converse_with_prompt(&system_prompt, &user_msg),
    )
    .await
    .ok()
    .flatten()?;

    // Reject if the suggestion is the same tool that failed
    if result.action == ctx.failed_action && result.tool_name == ctx.failed_tool {
        tracing::info!(
            tool = %ctx.failed_tool,
            "bedrock recovery suggested same tool — rejecting"
        );
        return None;
    }

    // Reject reply suggestions (means Bedrock couldn't find an alternative)
    if result.action == ActionKind::Reply {
        tracing::info!("bedrock recovery returned reply — no alternative found");
        return None;
    }

    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_recoverable ──────────────────────────────────────────

    #[test]
    fn recoverable_timeout() {
        assert!(is_recoverable("CAN bus timeout after 1000ms"));
    }

    #[test]
    fn recoverable_not_found() {
        assert!(is_recoverable("file not found: /var/log/syslog"));
    }

    #[test]
    fn recoverable_generic_error() {
        assert!(is_recoverable("unexpected EOF"));
    }

    #[test]
    fn not_recoverable_safety_violation() {
        assert!(!is_recoverable(
            "UDS safety violation: service 0x27 not allowed"
        ));
    }

    #[test]
    fn not_recoverable_blocked() {
        assert!(!is_recoverable("shell: command 'rm' is blocked"));
    }

    #[test]
    fn not_recoverable_injection() {
        assert!(!is_recoverable("shell: injection detected in command"));
    }

    #[test]
    fn not_recoverable_sensitive_path() {
        assert!(!is_recoverable("sensitive path: /etc/shadow"));
    }

    // ── rule_based_recovery: log tools ──────────────────────────

    fn make_ctx(
        tool: &str,
        action: ActionKind,
        args: serde_json::Value,
        error: &str,
    ) -> RecoveryContext {
        RecoveryContext {
            original_query: "search logs for errors".into(),
            failed_action: action,
            failed_tool: tool.into(),
            failed_args: args,
            error_message: error.into(),
            available_tools: vec![
                "search_logs".into(),
                "query_journal".into(),
                "read_dtcs".into(),
                "read_uds_dtcs".into(),
                "read_vin".into(),
                "read_uds_did".into(),
            ],
            elapsed_ms: 50,
        }
    }

    #[test]
    fn rule_syslog_to_journal() {
        let ctx = make_ctx(
            "search_logs",
            ActionKind::Tool,
            json!({"path": "/var/log/syslog", "query": "error"}),
            "file not found: /var/log/syslog",
        );
        let intent = rule_based_recovery(&ctx).unwrap();
        assert_eq!(intent.tool_name, "query_journal");
        assert_eq!(intent.action, ActionKind::Tool);
    }

    #[test]
    fn rule_tail_logs_to_journal() {
        let ctx = make_ctx(
            "tail_logs",
            ActionKind::Tool,
            json!({"path": "/var/log/syslog"}),
            "No such file or directory",
        );
        let intent = rule_based_recovery(&ctx).unwrap();
        assert_eq!(intent.tool_name, "query_journal");
    }

    #[test]
    fn rule_log_stats_to_journal() {
        let ctx = make_ctx(
            "log_stats",
            ActionKind::Tool,
            json!({"path": "/var/log/syslog"}),
            "file not found",
        );
        let intent = rule_based_recovery(&ctx).unwrap();
        assert_eq!(intent.tool_name, "query_journal");
    }

    #[test]
    fn rule_analyze_errors_to_journal() {
        let ctx = make_ctx(
            "analyze_errors",
            ActionKind::Tool,
            json!({"path": "/var/log/syslog"}),
            "No such file or directory",
        );
        let intent = rule_based_recovery(&ctx).unwrap();
        assert_eq!(intent.tool_name, "query_journal");
    }

    #[test]
    fn rule_journal_to_syslog() {
        let ctx = make_ctx(
            "query_journal",
            ActionKind::Tool,
            json!({"unit": "nginx"}),
            "journalctl: not found",
        );
        let intent = rule_based_recovery(&ctx).unwrap();
        assert_eq!(intent.tool_name, "search_logs");
        assert_eq!(intent.tool_args["path"], "/var/log/syslog");
    }

    // ── rule_based_recovery: CAN/UDS tools ──────────────────────

    #[test]
    fn rule_obd_dtcs_to_uds() {
        let ctx = make_ctx(
            "read_dtcs",
            ActionKind::Tool,
            json!({}),
            "CAN bus timeout after 1000ms",
        );
        let intent = rule_based_recovery(&ctx).unwrap();
        assert_eq!(intent.tool_name, "read_uds_dtcs");
        assert_eq!(intent.tool_args["ecu"], "BCR");
    }

    #[test]
    fn rule_uds_bcr_to_bcf() {
        let ctx = make_ctx(
            "read_uds_dtcs",
            ActionKind::Tool,
            json!({"ecu": "BCR"}),
            "UDS timeout: no response from ECU",
        );
        let intent = rule_based_recovery(&ctx).unwrap();
        assert_eq!(intent.tool_name, "read_uds_dtcs");
        assert_eq!(intent.tool_args["ecu"], "BCF");
    }

    #[test]
    fn rule_uds_bcf_timeout_no_recovery() {
        // BCF timeout has no further fallback
        let ctx = make_ctx(
            "read_uds_dtcs",
            ActionKind::Tool,
            json!({"ecu": "BCF"}),
            "UDS timeout: no response from ECU",
        );
        assert!(rule_based_recovery(&ctx).is_none());
    }

    #[test]
    fn rule_vin_to_uds_did() {
        let ctx = make_ctx(
            "read_vin",
            ActionKind::Tool,
            json!({}),
            "OBD-II Timeout waiting for response",
        );
        let intent = rule_based_recovery(&ctx).unwrap();
        assert_eq!(intent.tool_name, "read_uds_did");
        assert_eq!(intent.tool_args["ecu"], "BCR");
        assert_eq!(intent.tool_args["did"], 0xF190);
    }

    // ── rule_based_recovery: shell ──────────────────────────────

    #[test]
    fn rule_sensors_to_thermal() {
        let ctx = make_ctx(
            "sensors",
            ActionKind::Shell,
            json!({}),
            "shell: sensors: not found",
        );
        let intent = rule_based_recovery(&ctx).unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(
            intent.tool_name,
            "cat /sys/class/thermal/thermal_zone0/temp"
        );
    }

    // ── no rule match ───────────────────────────────────────────

    #[test]
    fn no_rule_for_unknown_tool() {
        let ctx = make_ctx(
            "can_monitor",
            ActionKind::Tool,
            json!({}),
            "CAN interface not available",
        );
        assert!(rule_based_recovery(&ctx).is_none());
    }

    #[test]
    fn no_rule_for_non_timeout_dtc_error() {
        let ctx = make_ctx(
            "read_dtcs",
            ActionKind::Tool,
            json!({}),
            "CAN interface not configured",
        );
        assert!(rule_based_recovery(&ctx).is_none());
    }

    #[test]
    fn no_rule_for_reply() {
        let ctx = RecoveryContext {
            original_query: "hello".into(),
            failed_action: ActionKind::Reply,
            failed_tool: String::new(),
            failed_args: json!({}),
            error_message: "something broke".into(),
            available_tools: vec![],
            elapsed_ms: 10,
        };
        assert!(rule_based_recovery(&ctx).is_none());
    }

    // ── build_recovery_prompt ───────────────────────────────────

    #[test]
    fn recovery_prompt_contains_context() {
        let ctx = make_ctx(
            "search_logs",
            ActionKind::Tool,
            json!({"path": "/var/log/syslog"}),
            "not found",
        );
        let (system, user) = build_recovery_prompt(&ctx);
        assert!(system.contains("search_logs"));
        assert!(system.contains("not found"));
        assert!(system.contains("Do NOT suggest \"search_logs\" again"));
        assert!(user.contains("search_logs"));
    }
}
