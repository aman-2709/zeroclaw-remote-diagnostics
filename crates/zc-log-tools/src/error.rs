//! Log analysis error types.

use thiserror::Error;

/// Errors that can occur during log analysis operations.
#[derive(Debug, Error)]
pub enum LogError {
    #[error("I/O error: {0}")]
    Io(String),

    #[error("parse error on line {line}: {message}")]
    Parse { line: usize, message: String },

    #[error("invalid log format: {0}")]
    Format(String),

    #[error("invalid regex pattern: {0}")]
    Regex(String),

    #[error("source not found: {0}")]
    NotFound(String),

    #[error("{0}")]
    Other(String),
}

/// Convenience alias for log analysis results.
pub type LogResult<T> = Result<T, LogError>;
