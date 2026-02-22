//! TLS configuration for mTLS connections to AWS IoT Core.
//!
//! Loads X.509 device certificate, private key, and CA certificate
//! from PEM files and configures rumqttc's TLS transport.

use rumqttc::Transport;

use crate::config::MqttConfig;
use crate::error::{MqttError, MqttResult};

/// Build a TLS transport from certificate file paths in the config.
///
/// Uses `TlsConfiguration::Simple` which reads PEM-encoded files:
/// - CA certificate (e.g., AmazonRootCA1.pem)
/// - Device certificate (X.509, issued by AWS IoT)
/// - Device private key
pub fn load_tls_transport(config: &MqttConfig) -> MqttResult<Transport> {
    let ca = std::fs::read(&config.ca_cert_path).map_err(|e| {
        MqttError::Tls(format!(
            "failed to read CA cert '{}': {e}",
            config.ca_cert_path
        ))
    })?;

    let client_cert = std::fs::read(&config.client_cert_path).map_err(|e| {
        MqttError::Tls(format!(
            "failed to read client cert '{}': {e}",
            config.client_cert_path
        ))
    })?;

    let client_key = std::fs::read(&config.client_key_path).map_err(|e| {
        MqttError::Tls(format!(
            "failed to read client key '{}': {e}",
            config.client_key_path
        ))
    })?;

    Ok(Transport::tls_with_config(
        rumqttc::TlsConfiguration::Simple {
            ca,
            alpn: None,
            client_auth: Some((client_cert, client_key)),
        },
    ))
}

/// Build MQTT options without TLS (for local testing / dev mode).
pub fn plaintext_transport() -> Transport {
    Transport::Tcp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_ca_cert_returns_error() {
        let config = MqttConfig {
            broker_host: "localhost".into(),
            broker_port: 1883,
            client_id: "test".into(),
            client_cert_path: "/nonexistent/cert.pem".into(),
            client_key_path: "/nonexistent/key.pem".into(),
            ca_cert_path: "/nonexistent/ca.pem".into(),
            keepalive_secs: 30,
        };
        let err = load_tls_transport(&config).err().expect("should fail");
        let msg = err.to_string();
        assert!(
            msg.contains("CA cert"),
            "error should mention CA cert: {msg}"
        );
    }
}
