//! Fleet agent configuration, loadable from TOML or environment.

use serde::Deserialize;
use zc_mqtt_channel::MqttConfig;

use crate::inference::OllamaConfig;

/// Top-level configuration for the fleet agent.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    /// Fleet this device belongs to.
    pub fleet_id: String,
    /// Unique device identifier (IoT Core thing name).
    pub device_id: String,
    /// MQTT connection settings.
    pub mqtt: MqttConfig,
    /// CAN bus interface name (e.g., "can0"). None disables CAN tools.
    #[serde(default)]
    pub can_interface: Option<String>,
    /// Heartbeat interval in seconds.
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_secs: u64,
    /// Log file paths to monitor (Phase 2: file watching).
    #[serde(default)]
    #[allow(dead_code)]
    pub log_paths: Vec<String>,
    /// Local Ollama inference settings. Optional — defaults to enabled.
    #[serde(default)]
    pub ollama: OllamaConfig,
}

fn default_heartbeat_interval() -> u64 {
    30
}

impl AgentConfig {
    /// Load config from a TOML file path.
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&contents)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_minimal_config() {
        let toml = r#"
fleet_id = "fleet-alpha"
device_id = "rpi-001"

[mqtt]
broker_host = "a1b2c3-ats.iot.us-east-1.amazonaws.com"
client_id = "rpi-001"
client_cert_path = "/etc/zeroclaw/cert.pem"
client_key_path = "/etc/zeroclaw/key.pem"
ca_cert_path = "/etc/zeroclaw/AmazonRootCA1.pem"
"#;
        let config: AgentConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.fleet_id, "fleet-alpha");
        assert_eq!(config.device_id, "rpi-001");
        assert_eq!(config.mqtt.broker_port, 8883); // default
        assert_eq!(config.heartbeat_interval_secs, 30); // default
        assert!(config.can_interface.is_none());
        assert!(config.log_paths.is_empty());
    }

    #[test]
    fn deserialize_full_config() {
        let toml = r#"
fleet_id = "fleet-beta"
device_id = "sbc-042"
can_interface = "can0"
heartbeat_interval_secs = 15
log_paths = ["/var/log/syslog", "/var/log/zeroclaw.log"]

[mqtt]
broker_host = "broker.example.com"
broker_port = 8883
client_id = "sbc-042"
client_cert_path = "/certs/cert.pem"
client_key_path = "/certs/key.pem"
ca_cert_path = "/certs/ca.pem"
keepalive_secs = 60
"#;
        let config: AgentConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.fleet_id, "fleet-beta");
        assert_eq!(config.can_interface.as_deref(), Some("can0"));
        assert_eq!(config.heartbeat_interval_secs, 15);
        assert_eq!(config.log_paths.len(), 2);
        assert_eq!(config.mqtt.keepalive_secs, 60);
    }

    #[test]
    fn deserialize_missing_ollama_uses_defaults() {
        let toml = r#"
fleet_id = "fleet-alpha"
device_id = "rpi-001"

[mqtt]
broker_host = "broker.example.com"
client_id = "rpi-001"
client_cert_path = "/certs/cert.pem"
client_key_path = "/certs/key.pem"
ca_cert_path = "/certs/ca.pem"
"#;
        let config: AgentConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.ollama.host, "http://localhost:11434");
        assert_eq!(config.ollama.model, "phi3:mini");
        assert_eq!(config.ollama.timeout_secs, 5);
        assert!(config.ollama.enabled);
    }

    #[test]
    fn deserialize_custom_ollama_config() {
        let toml = r#"
fleet_id = "fleet-alpha"
device_id = "rpi-001"

[mqtt]
broker_host = "broker.example.com"
client_id = "rpi-001"
client_cert_path = "/certs/cert.pem"
client_key_path = "/certs/key.pem"
ca_cert_path = "/certs/ca.pem"

[ollama]
host = "http://192.168.1.50:11434"
model = "gemma:2b"
timeout_secs = 10
enabled = false
"#;
        let config: AgentConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.ollama.host, "http://192.168.1.50:11434");
        assert_eq!(config.ollama.model, "gemma:2b");
        assert_eq!(config.ollama.timeout_secs, 10);
        assert!(!config.ollama.enabled);
    }
}
