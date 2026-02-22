//! Cloud API server configuration.

use serde::Deserialize;

/// Top-level API server configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiConfig {
    /// Listen address (e.g., "0.0.0.0").
    #[serde(default = "default_host")]
    pub host: String,
    /// Listen port.
    #[serde(default = "default_port")]
    pub port: u16,
    /// PostgreSQL connection URL (Phase 2).
    #[allow(dead_code)]
    pub database_url: Option<String>,
    /// Allowed CORS origins (e.g., ["http://localhost:5173"]).
    #[serde(default)]
    #[allow(dead_code)]
    pub cors_origins: Vec<String>,
    /// Enable AWS Bedrock cloud inference fallback (BEDROCK_ENABLED env var).
    #[serde(default)]
    pub bedrock_enabled: bool,
    /// Enable MQTT bridge (MQTT_ENABLED env var).
    #[serde(default)]
    pub mqtt_enabled: bool,
    /// MQTT broker host (MQTT_BROKER_HOST, default "localhost").
    #[serde(default = "default_mqtt_host")]
    pub mqtt_broker_host: String,
    /// MQTT broker port (MQTT_BROKER_PORT, default 1883).
    #[serde(default = "default_mqtt_port")]
    pub mqtt_broker_port: u16,
    /// Fleet ID for MQTT topic routing (MQTT_FLEET_ID, required when mqtt_enabled).
    #[serde(default)]
    pub mqtt_fleet_id: String,
    /// Use TLS for MQTT (MQTT_USE_TLS, default false — local mosquitto).
    #[serde(default)]
    pub mqtt_use_tls: bool,
    /// Path to CA certificate for MQTT TLS (MQTT_CA_CERT).
    pub mqtt_ca_cert: Option<String>,
    /// Path to client certificate for MQTT mTLS (MQTT_CLIENT_CERT).
    pub mqtt_client_cert: Option<String>,
    /// Path to client private key for MQTT mTLS (MQTT_CLIENT_KEY).
    pub mqtt_client_key: Option<String>,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    3000
}

fn default_mqtt_host() -> String {
    "localhost".to_string()
}

fn default_mqtt_port() -> u16 {
    1883
}

fn env_bool(key: &str) -> bool {
    std::env::var(key)
        .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
        .unwrap_or(false)
}

impl ApiConfig {
    /// Load config from environment variables.
    pub fn from_env() -> Self {
        Self {
            bedrock_enabled: env_bool("BEDROCK_ENABLED"),
            mqtt_enabled: env_bool("MQTT_ENABLED"),
            mqtt_broker_host: std::env::var("MQTT_BROKER_HOST")
                .unwrap_or_else(|_| default_mqtt_host()),
            mqtt_broker_port: std::env::var("MQTT_BROKER_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_mqtt_port()),
            mqtt_fleet_id: std::env::var("MQTT_FLEET_ID").unwrap_or_default(),
            mqtt_use_tls: env_bool("MQTT_USE_TLS"),
            mqtt_ca_cert: std::env::var("MQTT_CA_CERT").ok(),
            mqtt_client_cert: std::env::var("MQTT_CLIENT_CERT").ok(),
            mqtt_client_key: std::env::var("MQTT_CLIENT_KEY").ok(),
            ..Self::default()
        }
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            database_url: None,
            cors_origins: vec![],
            bedrock_enabled: false,
            mqtt_enabled: false,
            mqtt_broker_host: default_mqtt_host(),
            mqtt_broker_port: default_mqtt_port(),
            mqtt_fleet_id: String::new(),
            mqtt_use_tls: false,
            mqtt_ca_cert: None,
            mqtt_client_cert: None,
            mqtt_client_key: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = ApiConfig::default();
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 3000);
        assert!(config.database_url.is_none());
        assert!(!config.bedrock_enabled);
        assert!(!config.mqtt_enabled);
        assert_eq!(config.mqtt_broker_host, "localhost");
        assert_eq!(config.mqtt_broker_port, 1883);
    }
}
