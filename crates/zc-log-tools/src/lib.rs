//! Log analysis tools for ZeroClaw.
//!
//! Provides multi-format log parsing (syslog RFC 3164/5424, systemd journald,
//! newline-delimited JSON, plaintext), a `LogSource` abstraction for testability,
//! and 5 analysis tools: search_logs, analyze_errors, log_stats, tail_logs,
//! query_journal.

pub mod error;
pub mod mock;
pub mod parsers;
pub mod source;
pub mod tools;
pub mod types;

// Re-export key types for convenience
pub use error::{LogError, LogResult};
pub use mock::MockLogSource;
pub use source::{FileLogSource, LogSource};
pub use types::{LogEntry, LogFormat, LogSeverity, LogTool, ToolResult};
