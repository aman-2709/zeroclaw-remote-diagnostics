//! ZeroClaw Fleet Agent — edge runtime for connected vehicle diagnostics.
//!
//! Wires MQTT connectivity, CAN bus tools, and log analysis into a
//! single binary that runs on ARM edge devices.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tracing_subscriber::EnvFilter;

use zc_fleet_agent::config::AgentConfig;
use zc_fleet_agent::inference;
use zc_fleet_agent::registry::ToolRegistry;
use zc_fleet_agent::shadow_sync::{DeviceShadowState, SharedShadowState};
use zc_fleet_agent::{heartbeat, mqtt_loop, shadow_sync};
use zc_mqtt_channel::ShadowClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .json()
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "zc-fleet-agent starting"
    );

    // ── Load config ─────────────────────────────────────────────
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/etc/zeroclaw/agent.toml".to_string());

    let config = AgentConfig::from_file(&config_path)?;
    tracing::info!(
        fleet_id = %config.fleet_id,
        device_id = %config.device_id,
        "config loaded"
    );

    // ── Build tool registry ─────────────────────────────────────
    let registry = ToolRegistry::with_defaults();
    tracing::info!(tool_count = registry.len(), "tool registry initialized");

    // ── MQTT channel ────────────────────────────────────────────
    let (channel, eventloop) = if config.mqtt.use_tls {
        zc_mqtt_channel::MqttChannel::new(&config.mqtt, &config.fleet_id, &config.device_id)?
    } else {
        tracing::info!("MQTT plaintext mode (no TLS)");
        zc_mqtt_channel::MqttChannel::new_plaintext(
            &config.mqtt.broker_host,
            config.mqtt.broker_port,
            &config.mqtt.client_id,
            &config.fleet_id,
            &config.device_id,
        )
    };

    // Subscribe to inbound topics
    channel.subscribe_commands().await?;
    channel.subscribe_shadow_delta().await?;
    channel.subscribe_config().await?;
    tracing::info!("MQTT subscriptions active");

    // ── Ollama local inference ──────────────────────────────────
    let ollama_client = if config.ollama.enabled {
        tracing::info!(
            host = %config.ollama.host,
            model = %config.ollama.model,
            "ollama local inference enabled"
        );
        Some(inference::OllamaClient::new(config.ollama.clone()))
    } else {
        tracing::info!("ollama local inference disabled");
        None
    };
    let ollama_ref = ollama_client.as_ref();

    // ── CAN interface (mock for now — real socketcan in Phase 2) ─
    let can_interface = zc_canbus_tools::MockCanInterface::new();
    let can_available = config.can_interface.is_some();

    // ── Log source ──────────────────────────────────────────────
    let log_source = zc_log_tools::FileLogSource;

    // ── Shadow state ────────────────────────────────────────────
    let shadow_state: SharedShadowState = Arc::new(RwLock::new(DeviceShadowState {
        tool_count: registry.len(),
        can_status: if can_available {
            "running".to_string()
        } else {
            "stopped".to_string()
        },
        ollama_status: if config.ollama.enabled {
            "enabled".to_string()
        } else {
            "disabled".to_string()
        },
        ..Default::default()
    }));

    let shadow_client = ShadowClient::new(&channel, &config.fleet_id, &config.device_id);

    // ── Start background tasks ──────────────────────────────────
    let start_time = tokio::time::Instant::now();

    tracing::info!("zc-fleet-agent ready");

    tokio::select! {
        // Drive the MQTT event loop + dispatch commands
        () = mqtt_loop::run(eventloop, &channel, &registry, &can_interface, &log_source, ollama_ref, &shadow_state) => {
            tracing::error!("MQTT loop exited unexpectedly");
        }
        // Publish periodic heartbeats
        () = heartbeat::run(
            &channel,
            Duration::from_secs(config.heartbeat_interval_secs),
            start_time,
            can_available,
        ) => {
            tracing::error!("heartbeat loop exited unexpectedly");
        }
        // Periodic shadow state sync
        () = shadow_sync::run(
            &shadow_client,
            &shadow_state,
            Duration::from_secs(config.shadow_sync_interval_secs),
            start_time,
        ) => {
            tracing::error!("shadow sync loop exited unexpectedly");
        }
        // Graceful shutdown on SIGINT/SIGTERM
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("shutdown signal received");
        }
    }

    tracing::info!("zc-fleet-agent stopped");
    Ok(())
}
