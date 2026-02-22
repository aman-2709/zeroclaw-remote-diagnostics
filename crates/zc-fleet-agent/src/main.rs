//! ZeroClaw Fleet Agent — edge runtime for connected vehicle diagnostics.
//!
//! Wires MQTT connectivity, CAN bus tools, and log analysis into a
//! single binary that runs on ARM edge devices.

mod config;
mod executor;
mod heartbeat;
mod mqtt_loop;
mod registry;

use std::time::Duration;

use tracing_subscriber::EnvFilter;

use crate::config::AgentConfig;
use crate::registry::ToolRegistry;

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
    let (channel, eventloop) =
        zc_mqtt_channel::MqttChannel::new(&config.mqtt, &config.fleet_id, &config.device_id)?;

    // Subscribe to inbound topics
    channel.subscribe_commands().await?;
    channel.subscribe_shadow_delta().await?;
    channel.subscribe_config().await?;
    tracing::info!("MQTT subscriptions active");

    // ── CAN interface (mock for now — real socketcan in Phase 2) ─
    let can_interface = zc_canbus_tools::MockCanInterface::new();
    let can_available = config.can_interface.is_some();

    // ── Log source ──────────────────────────────────────────────
    let log_source = zc_log_tools::FileLogSource;

    // ── Start background tasks ──────────────────────────────────
    let start_time = tokio::time::Instant::now();

    tracing::info!("zc-fleet-agent ready");

    tokio::select! {
        // Drive the MQTT event loop + dispatch commands
        () = mqtt_loop::run(eventloop, &channel, &registry, &can_interface, &log_source) => {
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
        // Graceful shutdown on SIGINT/SIGTERM
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("shutdown signal received");
        }
    }

    tracing::info!("zc-fleet-agent stopped");
    Ok(())
}
