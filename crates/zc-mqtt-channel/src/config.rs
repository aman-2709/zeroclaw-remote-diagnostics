use serde::Deserialize;

/// MQTT connection configuration, loadable from TOML or environment.
#[derive(Debug, Clone, Deserialize)]
pub struct MqttConfig {
    /// MQTT broker hostname (e.g., AWS IoT endpoint).
    pub broker_host: String,
    /// MQTT broker port (default 8883 for TLS).
    #[serde(default = "default_port")]
    pub broker_port: u16,
    /// MQTT client ID (should be unique per device).
    pub client_id: String,
    /// Enable TLS (mTLS). When false, connects plaintext (local dev).
    #[serde(default = "default_use_tls")]
    pub use_tls: bool,
    /// Path to device X.509 certificate (PEM).
    #[serde(default)]
    pub client_cert_path: String,
    /// Path to device private key (PEM).
    #[serde(default)]
    pub client_key_path: String,
    /// Path to CA certificate (e.g., AmazonRootCA1.pem).
    #[serde(default)]
    pub ca_cert_path: String,
    /// Keep-alive interval in seconds.
    #[serde(default = "default_keepalive")]
    pub keepalive_secs: u16,
}

fn default_use_tls() -> bool {
    true
}

fn default_port() -> u16 {
    8883
}

fn default_keepalive() -> u16 {
    30
}
