//! Periodic shadow state reporter.
//!
//! Reports the device's current state as a shadow update at a configurable
//! interval, allowing the cloud to maintain an up-to-date view of the device.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::RwLock;
use tokio::time;

use zc_mqtt_channel::ShadowClient;
use zc_mqtt_channel::channel::Channel;

/// Device-side shadow state reported to the cloud.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceShadowState {
    pub agent_version: String,
    pub uptime_secs: u64,
    pub can_status: String,
    pub ollama_status: String,
    pub tool_count: usize,
    pub last_command_id: Option<String>,
    pub last_command_tool: Option<String>,
    pub last_command_at: Option<String>,
}

/// Shared shadow state that can be updated from the mqtt_loop.
pub type SharedShadowState = Arc<RwLock<DeviceShadowState>>;

impl Default for DeviceShadowState {
    fn default() -> Self {
        Self {
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_secs: 0,
            can_status: "unknown".to_string(),
            ollama_status: "unknown".to_string(),
            tool_count: 0,
            last_command_id: None,
            last_command_tool: None,
            last_command_at: None,
        }
    }
}

/// Run the shadow sync loop, reporting state at `interval`.
///
/// Reports immediately on boot, then at the configured interval.
pub async fn run<C: Channel>(
    shadow_client: &ShadowClient<'_, C>,
    shadow_state: &SharedShadowState,
    interval: Duration,
    start_time: tokio::time::Instant,
) {
    let mut version: u64 = 0;

    // Report immediately on boot.
    version += 1;
    report_state(shadow_client, shadow_state, start_time, version).await;

    let mut ticker = time::interval(interval);
    // Skip the first tick (fires immediately).
    ticker.tick().await;

    loop {
        ticker.tick().await;
        version += 1;
        report_state(shadow_client, shadow_state, start_time, version).await;
    }
}

async fn report_state<C: Channel>(
    shadow_client: &ShadowClient<'_, C>,
    shadow_state: &SharedShadowState,
    start_time: tokio::time::Instant,
    version: u64,
) {
    let mut state = shadow_state.write().await;
    state.uptime_secs = start_time.elapsed().as_secs();
    let reported = match serde_json::to_value(&*state) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, "failed to serialize shadow state");
            return;
        }
    };
    drop(state);

    if let Err(e) = shadow_client
        .report_state("diagnostics", reported, version)
        .await
    {
        tracing::warn!(error = %e, "failed to publish shadow update");
    } else {
        tracing::debug!(version = version, "shadow state reported");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zc_mqtt_channel::MockChannel;
    use zc_protocol::shadows::ShadowUpdate;

    fn make_shadow_state(tool_count: usize) -> SharedShadowState {
        Arc::new(RwLock::new(DeviceShadowState {
            tool_count,
            ..Default::default()
        }))
    }

    #[tokio::test]
    async fn initial_report_published() {
        let mock = MockChannel::new();
        let client = ShadowClient::new(&mock, "fleet-alpha", "rpi-001");
        let state = make_shadow_state(9);
        let start = tokio::time::Instant::now();

        report_state(&client, &state, start, 1).await;

        let msgs = mock.published();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].topic, "fleet/fleet-alpha/rpi-001/shadow/update");
    }

    #[tokio::test]
    async fn report_contains_expected_fields() {
        let mock = MockChannel::new();
        let client = ShadowClient::new(&mock, "fleet-alpha", "rpi-001");
        let state = make_shadow_state(9);
        let start = tokio::time::Instant::now();

        report_state(&client, &state, start, 1).await;

        let msgs = mock.published();
        let update: ShadowUpdate = serde_json::from_slice(&msgs[0].payload).unwrap();
        assert_eq!(update.shadow_name, "diagnostics");
        assert_eq!(update.reported["tool_count"], 9);
        assert!(update.reported.get("agent_version").is_some());
        assert!(update.reported.get("uptime_secs").is_some());
    }

    #[tokio::test]
    async fn version_increments_on_reports() {
        let mock = MockChannel::new();
        let client = ShadowClient::new(&mock, "fleet-alpha", "rpi-001");
        let state = make_shadow_state(9);
        let start = tokio::time::Instant::now();

        report_state(&client, &state, start, 1).await;
        report_state(&client, &state, start, 2).await;

        let msgs = mock.published();
        assert_eq!(msgs.len(), 2);
        let u1: ShadowUpdate = serde_json::from_slice(&msgs[0].payload).unwrap();
        let u2: ShadowUpdate = serde_json::from_slice(&msgs[1].payload).unwrap();
        assert_eq!(u1.version, 1);
        assert_eq!(u2.version, 2);
    }
}
