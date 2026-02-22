//! MQTT channel error types.

use thiserror::Error;

/// Errors that can occur during MQTT operations.
#[derive(Debug, Error)]
pub enum MqttError {
    #[error("connection error: {0}")]
    Connection(String),

    #[error("publish error: {0}")]
    Publish(String),

    #[error("subscribe error: {0}")]
    Subscribe(String),

    #[error("TLS error: {0}")]
    Tls(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("I/O error: {0}")]
    Io(String),

    #[error("{0}")]
    Other(String),
}

/// Convenience alias for MQTT results.
pub type MqttResult<T> = Result<T, MqttError>;
