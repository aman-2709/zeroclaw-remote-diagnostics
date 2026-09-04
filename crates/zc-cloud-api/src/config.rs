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
    /// Allowed CORS origins (comma-separated CORS_ORIGINS).
    #[serde(default)]
    pub cors_origins: Vec<String>,
    /// Optional bearer token protecting all `/api/v1` routes.
    #[serde(default)]
    pub api_auth_token: Option<String>,
    /// Explicit escape hatch for local plaintext/insecure development only.
    #[serde(default)]
    pub allow_insecure: bool,
    /// Maximum request body size in bytes.
    #[serde(default = "default_max_body_bytes")]
    pub max_body_bytes: usize,
    /// Inference engine selection: "local" (rule-based), "bedrock" (cloud LLM),
    /// or "tiered" (local rules first, bedrock fallback).
    /// Set via INFERENCE_ENGINE env var. Defaults to "local".
    #[serde(default = "default_inference_engine")]
    pub inference_engine: String,
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
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    3000
}

fn default_max_body_bytes() -> usize {
    1024 * 1024
}

fn default_inference_engine() -> String {
    "local".to_string()
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
            host: std::env::var("HOST").unwrap_or_else(|_| default_host()),
            port: std::env::var("PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default_port()),
            cors_origins: std::env::var("CORS_ORIGINS")
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|origin| !origin.is_empty())
                .map(str::to_owned)
                .collect(),
            api_auth_token: std::env::var("API_AUTH_TOKEN")
                .ok()
                .filter(|token| !token.trim().is_empty()),
            allow_insecure: env_bool("ALLOW_INSECURE"),
            max_body_bytes: std::env::var("MAX_BODY_BYTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(default_max_body_bytes),
            inference_engine: std::env::var("INFERENCE_ENGINE")
                .unwrap_or_else(|_| default_inference_engine()),
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

    /// Validate settings that could expose the API or device channel insecurely.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_body_bytes == 0 {
            return Err("MAX_BODY_BYTES must be greater than zero".into());
        }

        if let Some(token) = &self.api_auth_token {
            if token.len() < 32 {
                return Err("API_AUTH_TOKEN must be at least 32 characters".into());
            }
            if token.chars().any(char::is_whitespace) {
                return Err("API_AUTH_TOKEN must not contain whitespace".into());
            }
        }

        let local_bind = self
            .host
            .parse::<std::net::IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or_else(|_| {
                matches!(self.host.as_str(), "localhost" | "localhost.localdomain")
            });
        if !local_bind && self.api_auth_token.is_none() && !self.allow_insecure {
            return Err(
                "non-loopback HOST requires API_AUTH_TOKEN (or explicit ALLOW_INSECURE=true)"
                    .into(),
            );
        }
        if !local_bind && self.cors_origins.is_empty() && !self.allow_insecure {
            return Err(
                "non-loopback HOST requires CORS_ORIGINS (or explicit ALLOW_INSECURE=true)".into(),
            );
        }
        if self.mqtt_enabled && !self.mqtt_use_tls && !self.allow_insecure {
            return Err(
                "MQTT_USE_TLS=true is required when MQTT is enabled (or explicit ALLOW_INSECURE=true)".into(),
            );
        }

        Ok(())
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            database_url: None,
            cors_origins: vec![],
            api_auth_token: None,
            allow_insecure: false,
            max_body_bytes: default_max_body_bytes(),
            inference_engine: default_inference_engine(),
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
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 3000);
        assert_eq!(config.max_body_bytes, 1024 * 1024);
        assert!(config.api_auth_token.is_none());
        assert!(config.database_url.is_none());
        assert_eq!(config.inference_engine, "local");
        assert!(!config.mqtt_enabled);
        assert_eq!(config.mqtt_broker_host, "localhost");
        assert_eq!(config.mqtt_broker_port, 1883);
    }

    #[test]
    fn non_loopback_requires_authentication() {
        let config = ApiConfig {
            host: "0.0.0.0".into(),
            ..ApiConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn insecure_local_development_requires_explicit_opt_in_for_plaintext_mqtt() {
        let config = ApiConfig {
            mqtt_enabled: true,
            ..ApiConfig::default()
        };
        assert!(config.validate().is_err());

        let allowed = ApiConfig {
            mqtt_enabled: true,
            allow_insecure: true,
            ..ApiConfig::default()
        };
        assert!(allowed.validate().is_ok());
    }

    #[test]
    fn non_loopback_requires_explicit_cors_origins() {
        let config = ApiConfig {
            host: "0.0.0.0".into(),
            api_auth_token: Some("test-token-with-at-least-32-characters".into()),
            ..ApiConfig::default()
        };
        assert!(config.validate().is_err());

        let allowed = ApiConfig {
            cors_origins: vec!["https://console.example.com".into()],
            ..config
        };
        assert!(allowed.validate().is_ok());
    }

    #[test]
    fn short_auth_token_is_rejected() {
        let config = ApiConfig {
            api_auth_token: Some("too-short".into()),
            ..ApiConfig::default()
        };
        assert!(config.validate().is_err());
    }
}
