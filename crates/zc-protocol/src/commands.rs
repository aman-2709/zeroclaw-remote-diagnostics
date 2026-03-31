use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Envelope wrapping a command sent from cloud to device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEnvelope {
    /// Unique command ID (UUIDv7 for time-sortability).
    pub id: Uuid,
    /// Fleet the target device belongs to.
    pub fleet_id: String,
    /// Target device identifier.
    pub device_id: String,
    /// Original natural-language input from the operator.
    pub natural_language: String,
    /// Parsed intent (set by inference engine, may be absent on initial send).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed_intent: Option<ParsedIntent>,
    /// Correlation ID for request/response matching.
    pub correlation_id: Uuid,
    /// Who initiated this command.
    pub initiated_by: String,
    /// When the command was created.
    pub created_at: DateTime<Utc>,
    /// Command timeout in seconds (default 30).
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u32,
    /// Name of the inference engine that parsed the command (e.g. "local", "bedrock").
    /// Set by the cloud when it pre-parses; None if unparsed or edge-parsed.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub engine: Option<String>,
}

fn default_timeout_secs() -> u32 {
    30
}

/// What kind of action the parsed intent represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// Invoke one of the 9 registered tools (CAN bus + log).
    #[default]
    Tool,
    /// Execute a safe shell command on the device.
    Shell,
    /// Return a conversational reply (no tool or shell execution).
    Reply,
    /// Execute a step and continue the agentic loop (multi-step reasoning).
    Continue,
}

/// Parsed intent extracted from natural language by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedIntent {
    /// What kind of action to take.
    #[serde(default)]
    pub action: ActionKind,
    /// Tool to invoke (e.g., "read_dtcs", "read_pid").
    /// For Shell actions, contains the shell command string.
    /// For Reply actions, may be empty.
    pub tool_name: String,
    /// Arguments for the tool as key-value pairs.
    /// For Reply actions, may contain a "message" key with the reply text.
    #[serde(default)]
    pub tool_args: serde_json::Value,
    /// LLM confidence score (0.0 - 1.0).
    pub confidence: f64,
    /// LLM reasoning for this step (agentic loop only).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reasoning: Option<String>,
}

/// Which recovery mechanism produced the retry intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoverySource {
    /// Deterministic rule mapping (free, <1ms).
    RuleBased,
    /// Local Ollama LLM suggestion.
    Ollama,
    /// AWS Bedrock cloud LLM suggestion.
    Bedrock,
}

/// Summary of a single execution attempt within the recovery loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptSummary {
    /// 1-based attempt number.
    pub attempt: u8,
    /// What kind of action was attempted.
    pub action: ActionKind,
    /// Tool name or shell command.
    pub tool_name: String,
    /// Arguments passed to the tool.
    pub tool_args: serde_json::Value,
    /// Whether this attempt succeeded.
    pub success: bool,
    /// Error message if the attempt failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Duration of this attempt in milliseconds.
    pub duration_ms: u64,
    /// How the recovery intent was produced (None for the first attempt).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_source: Option<RecoverySource>,
}

/// Summary of a single step in the agentic reasoning loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepSummary {
    /// 1-based step number.
    pub step: u8,
    /// What kind of action was executed.
    pub action: ActionKind,
    /// Tool name or shell command.
    pub tool_name: String,
    /// Arguments passed to the tool.
    pub tool_args: serde_json::Value,
    /// Whether this step succeeded.
    pub success: bool,
    /// Truncated output summary (max 2KB per step).
    pub output_summary: String,
    /// LLM reasoning for choosing this step.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// Duration of this step in milliseconds.
    pub duration_ms: u64,
}

/// Response from device back to cloud after executing a command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResponse {
    /// ID of the original command.
    pub command_id: Uuid,
    /// Correlation ID matching the request.
    pub correlation_id: Uuid,
    /// Device that executed the command.
    pub device_id: String,
    /// Current command status.
    pub status: CommandStatus,
    /// Which inference tier handled the command.
    pub inference_tier: InferenceTier,
    /// Human-readable response text (LLM-generated summary).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_text: Option<String>,
    /// Structured response data (tool output).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_data: Option<serde_json::Value>,
    /// Processing latency in milliseconds.
    pub latency_ms: u64,
    /// When the response was generated.
    pub responded_at: DateTime<Utc>,
    /// Error message if status is Failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Recovery attempt chain (present only when >1 attempt was made).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub attempts: Option<Vec<AttemptSummary>>,
    /// Name of the inference engine that parsed the command (e.g. "ollama", "bedrock", "fallback").
    /// None when the intent was pre-parsed by the cloud.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub engine: Option<String>,
    /// Agentic loop step chain (present only for multi-step commands).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub steps: Option<Vec<StepSummary>>,
}

/// Lifecycle status of a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
    Pending,
    Sent,
    Processing,
    Completed,
    Failed,
    Timeout,
    Cancelled,
}

/// Which inference engine handled the query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceTier {
    /// Local rule-based engine, Ollama, or FallbackReply (no cloud cost).
    Local,
    /// AWS Bedrock Nova Lite (cloud or edge).
    CloudLite,
    /// AWS Bedrock Claude Haiku.
    CloudHaiku,
    /// AWS Bedrock Claude Sonnet (escalation only).
    CloudSonnet,
}

impl InferenceTier {
    /// Derive the inference tier from an engine name string.
    ///
    /// Engine names: `"local"` / `"rule-based"` / `"ollama"` / `"fallback"` → Local,
    /// `"bedrock"` → CloudLite.
    pub fn from_engine_name(name: &str) -> Self {
        match name {
            "bedrock" => Self::CloudLite,
            _ => Self::Local,
        }
    }
}

impl CommandEnvelope {
    pub fn new(
        fleet_id: impl Into<String>,
        device_id: impl Into<String>,
        natural_language: impl Into<String>,
        initiated_by: impl Into<String>,
    ) -> Self {
        let id = Uuid::now_v7();
        Self {
            id,
            fleet_id: fleet_id.into(),
            device_id: device_id.into(),
            natural_language: natural_language.into(),
            parsed_intent: None,
            correlation_id: id,
            initiated_by: initiated_by.into(),
            created_at: Utc::now(),
            timeout_secs: default_timeout_secs(),
            engine: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_envelope_roundtrip() {
        let cmd = CommandEnvelope::new("fleet-alpha", "rpi-001", "read DTCs", "operator@test.com");
        let json = serde_json::to_string(&cmd).unwrap();
        let deserialized: CommandEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.fleet_id, "fleet-alpha");
        assert_eq!(deserialized.device_id, "rpi-001");
        assert_eq!(deserialized.natural_language, "read DTCs");
        assert_eq!(deserialized.timeout_secs, 30);
    }

    #[test]
    fn command_status_serialization() {
        let status = CommandStatus::Completed;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, r#""completed""#);
    }

    #[test]
    fn inference_tier_serialization() {
        let tier = InferenceTier::CloudHaiku;
        let json = serde_json::to_string(&tier).unwrap();
        assert_eq!(json, r#""cloud_haiku""#);
    }

    #[test]
    fn action_kind_default_is_tool() {
        assert_eq!(ActionKind::default(), ActionKind::Tool);
    }

    #[test]
    fn action_kind_serialization() {
        assert_eq!(
            serde_json::to_string(&ActionKind::Tool).unwrap(),
            r#""tool""#
        );
        assert_eq!(
            serde_json::to_string(&ActionKind::Shell).unwrap(),
            r#""shell""#
        );
        assert_eq!(
            serde_json::to_string(&ActionKind::Reply).unwrap(),
            r#""reply""#
        );
    }

    #[test]
    fn parsed_intent_backward_compat_no_action_field() {
        // Old JSON without "action" field should deserialize with default (Tool)
        let json = r#"{"tool_name": "read_dtcs", "tool_args": {}, "confidence": 0.95}"#;
        let intent: ParsedIntent = serde_json::from_str(json).unwrap();
        assert_eq!(intent.action, ActionKind::Tool);
        assert_eq!(intent.tool_name, "read_dtcs");
    }

    #[test]
    fn parsed_intent_with_shell_action() {
        let json =
            r#"{"action": "shell", "tool_name": "uname -a", "tool_args": {}, "confidence": 0.9}"#;
        let intent: ParsedIntent = serde_json::from_str(json).unwrap();
        assert_eq!(intent.action, ActionKind::Shell);
        assert_eq!(intent.tool_name, "uname -a");
    }

    #[test]
    fn parsed_intent_with_reply_action() {
        let json = r#"{"action": "reply", "tool_name": "", "tool_args": {"message": "I'm doing well!"}, "confidence": 1.0}"#;
        let intent: ParsedIntent = serde_json::from_str(json).unwrap();
        assert_eq!(intent.action, ActionKind::Reply);
        assert_eq!(intent.tool_args["message"], "I'm doing well!");
    }

    #[test]
    fn command_response_with_error() {
        let resp = CommandResponse {
            command_id: Uuid::now_v7(),
            correlation_id: Uuid::now_v7(),
            device_id: "rpi-001".into(),
            status: CommandStatus::Failed,
            inference_tier: InferenceTier::Local,
            response_text: None,
            response_data: None,
            latency_ms: 50,
            responded_at: Utc::now(),
            error: Some("CAN bus interface not available".into()),
            attempts: None,
            engine: None,
            steps: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("CAN bus interface not available"));
        assert!(!json.contains("response_text")); // skip_serializing_if = None
        assert!(!json.contains("attempts")); // skip_serializing_if = None
        assert!(!json.contains("engine")); // skip_serializing_if = None
    }

    #[test]
    fn attempt_summary_roundtrip() {
        let attempt = AttemptSummary {
            attempt: 1,
            action: ActionKind::Tool,
            tool_name: "search_logs".into(),
            tool_args: serde_json::json!({"path": "/var/log/syslog"}),
            success: false,
            error: Some("No such file or directory".into()),
            duration_ms: 45,
            recovery_source: None,
        };
        let json = serde_json::to_string(&attempt).unwrap();
        let deserialized: AttemptSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.attempt, 1);
        assert!(!deserialized.success);
        assert!(deserialized.recovery_source.is_none());
    }

    #[test]
    fn attempt_summary_with_recovery_source() {
        let attempt = AttemptSummary {
            attempt: 2,
            action: ActionKind::Tool,
            tool_name: "query_journal".into(),
            tool_args: serde_json::json!({"unit": "syslog"}),
            success: true,
            error: None,
            duration_ms: 120,
            recovery_source: Some(RecoverySource::RuleBased),
        };
        let json = serde_json::to_string(&attempt).unwrap();
        assert!(json.contains("rule_based"));
        let deserialized: AttemptSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized.recovery_source,
            Some(RecoverySource::RuleBased)
        );
    }

    #[test]
    fn command_response_with_attempts_roundtrip() {
        let resp = CommandResponse {
            command_id: Uuid::now_v7(),
            correlation_id: Uuid::now_v7(),
            device_id: "rpi-001".into(),
            status: CommandStatus::Completed,
            inference_tier: InferenceTier::Local,
            response_text: Some("Found 3 journal entries".into()),
            response_data: None,
            latency_ms: 165,
            responded_at: Utc::now(),
            error: None,
            attempts: Some(vec![
                AttemptSummary {
                    attempt: 1,
                    action: ActionKind::Tool,
                    tool_name: "search_logs".into(),
                    tool_args: serde_json::json!({"path": "/var/log/syslog"}),
                    success: false,
                    error: Some("not found".into()),
                    duration_ms: 45,
                    recovery_source: None,
                },
                AttemptSummary {
                    attempt: 2,
                    action: ActionKind::Tool,
                    tool_name: "query_journal".into(),
                    tool_args: serde_json::json!({"unit": "syslog"}),
                    success: true,
                    error: None,
                    duration_ms: 120,
                    recovery_source: Some(RecoverySource::RuleBased),
                },
            ]),
            engine: Some("ollama".into()),
            steps: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("attempts"));
        assert!(json.contains("rule_based"));
        assert!(json.contains(r#""engine":"ollama""#));
        let deserialized: CommandResponse = serde_json::from_str(&json).unwrap();
        let attempts = deserialized.attempts.unwrap();
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].tool_name, "search_logs");
        assert_eq!(attempts[1].tool_name, "query_journal");
        assert_eq!(deserialized.engine.as_deref(), Some("ollama"));
    }

    #[test]
    fn command_response_backward_compat_no_attempts() {
        // Old JSON without "attempts" field should deserialize with None
        let json = r#"{
            "command_id": "00000000-0000-0000-0000-000000000000",
            "correlation_id": "00000000-0000-0000-0000-000000000000",
            "device_id": "rpi-001",
            "status": "completed",
            "inference_tier": "local",
            "latency_ms": 50,
            "responded_at": "2025-01-01T00:00:00Z"
        }"#;
        let resp: CommandResponse = serde_json::from_str(json).unwrap();
        assert!(resp.attempts.is_none());
    }

    #[test]
    fn recovery_source_serialization() {
        assert_eq!(
            serde_json::to_string(&RecoverySource::RuleBased).unwrap(),
            r#""rule_based""#
        );
        assert_eq!(
            serde_json::to_string(&RecoverySource::Ollama).unwrap(),
            r#""ollama""#
        );
        assert_eq!(
            serde_json::to_string(&RecoverySource::Bedrock).unwrap(),
            r#""bedrock""#
        );
    }

    #[test]
    fn recovery_source_bedrock_roundtrip() {
        let json = r#""bedrock""#;
        let source: RecoverySource = serde_json::from_str(json).unwrap();
        assert_eq!(source, RecoverySource::Bedrock);
    }
}
