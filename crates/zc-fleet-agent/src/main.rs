use tracing_subscriber::EnvFilter;

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

    // TODO Phase 1 Week 3: Wire ZeroClaw runtime with:
    // - MQTT channel (zc-mqtt-channel)
    // - CAN bus tools (zc-canbus-tools)
    // - Log analysis tools (zc-log-tools)
    // - Ollama provider configuration
    // - Health check endpoint

    tracing::info!("zc-fleet-agent ready (skeleton — no runtime wired yet)");
    Ok(())
}
