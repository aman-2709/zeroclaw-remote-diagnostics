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
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    3000
}

impl ApiConfig {
    /// Load config from environment variables.
    pub fn from_env() -> Self {
        let bedrock_enabled = std::env::var("BEDROCK_ENABLED")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false);
        Self {
            bedrock_enabled,
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
    }
}
