//! Core log analysis types and the LogTool trait.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::LogResult;

// ── Log Severity ──────────────────────────────────────────────

/// Log severity level, ordered from least to most severe.
///
/// Variant declaration order matters — `#[derive(Ord)]` uses it,
/// so Debug < Info < Notice < Warning < Error < Critical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogSeverity {
    Debug,
    Info,
    Notice,
    Warning,
    Error,
    Critical,
}

impl LogSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Notice => "notice",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::Critical => "critical",
        }
    }

    /// Map syslog numeric severity (0–7) to `LogSeverity`.
    pub fn from_syslog_severity(sev: u8) -> Self {
        match sev {
            0..=2 => Self::Critical, // Emergency, Alert, Critical
            3 => Self::Error,
            4 => Self::Warning,
            5 => Self::Notice,
            6 => Self::Info,
            _ => Self::Debug, // 7 or unknown
        }
    }
}

impl std::fmt::Display for LogSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── Log Format ────────────────────────────────────────────────

/// Supported log format types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    /// BSD syslog (RFC 3164).
    Syslog3164,
    /// IETF syslog (RFC 5424).
    Syslog5424,
    /// systemd journal export format.
    Journald,
    /// Newline-delimited JSON.
    JsonLines,
    /// Unstructured plaintext.
    Plaintext,
}

// ── Log Entry ─────────────────────────────────────────────────

/// A parsed log entry, normalized from any supported format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Parsed timestamp (None if unparseable).
    pub timestamp: Option<DateTime<Utc>>,
    /// Severity level.
    pub severity: LogSeverity,
    /// Source identifier (hostname, service name, app, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Log message body.
    pub message: String,
    /// Original raw line(s).
    pub raw: String,
    /// 1-based line number in the source.
    pub line_number: usize,
    /// Format this entry was parsed from.
    pub format: LogFormat,
    /// Additional structured fields (from JSON or journald).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub fields: HashMap<String, String>,
}

// ── Tool Result ───────────────────────────────────────────────

/// Result of executing a log analysis tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Tool name that produced this result.
    pub tool_name: String,
    /// Whether the tool execution succeeded.
    pub success: bool,
    /// Structured result data (JSON).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// Human-readable summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Error message if success is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ToolResult {
    pub fn success(
        tool_name: impl Into<String>,
        data: serde_json::Value,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            tool_name: tool_name.into(),
            success: true,
            data: Some(data),
            summary: Some(summary.into()),
            error: None,
        }
    }

    pub fn failure(tool_name: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            success: false,
            data: None,
            summary: None,
            error: Some(error.into()),
        }
    }
}

// ── LogTool Trait ─────────────────────────────────────────────

/// Trait for log analysis tools.
///
/// Mirrors the `CanTool` pattern — trivially wrappable via thin adapter
/// when wiring into the fleet agent.
#[async_trait]
pub trait LogTool: Send + Sync {
    /// Tool name (e.g., "search_logs").
    fn name(&self) -> &str;

    /// Human-readable description.
    fn description(&self) -> &str;

    /// JSON Schema describing accepted arguments.
    fn parameters_schema(&self) -> serde_json::Value;

    /// Execute the tool with JSON arguments against a log source.
    async fn execute(
        &self,
        args: serde_json::Value,
        source: &dyn crate::source::LogSource,
    ) -> LogResult<ToolResult>;
}
