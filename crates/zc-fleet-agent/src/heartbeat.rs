//! Periodic heartbeat publisher.
//!
//! Sends a `Heartbeat` message at a configurable interval so the cloud
//! knows the device is alive.

use std::time::Duration;

use chrono::Utc;
use tokio::time;

use zc_mqtt_channel::MqttChannel;
use zc_protocol::device::{DeviceStatus, Heartbeat, ServiceStatus};

/// Run the heartbeat loop, publishing at `interval`.
///
/// This function runs forever until the task is cancelled. Intended
/// to be spawned as a background tokio task.
pub async fn run(
    channel: &MqttChannel,
    interval: Duration,
    start_time: tokio::time::Instant,
    can_available: bool,
) {
    let mut ticker = time::interval(interval);
    // Skip the first tick (fires immediately).
    ticker.tick().await;

    loop {
        ticker.tick().await;

        let heartbeat = Heartbeat {
            device_id: channel.device_id().to_string(),
            fleet_id: channel.fleet_id().to_string(),
            status: DeviceStatus::Online,
            uptime_secs: start_time.elapsed().as_secs(),
            ollama_status: ServiceStatus::Unknown, // Phase 2
            can_status: if can_available {
                ServiceStatus::Running
            } else {
                ServiceStatus::Stopped
            },
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
            timestamp: Utc::now(),
        };

        if let Err(e) = channel.publish_heartbeat(&heartbeat).await {
            tracing::warn!(error = %e, "failed to publish heartbeat");
        } else {
            tracing::debug!(uptime_secs = heartbeat.uptime_secs, "heartbeat sent");
        }
    }
}
