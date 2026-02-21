use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .json()
        .init();

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "zc-cloud-api starting");

    // TODO Phase 2 Week 6: Wire Axum server with:
    // - PostgreSQL connection pool (sqlx)
    // - REST API routes (devices, commands, telemetry, fleets, audit)
    // - WebSocket hub for real-time updates
    // - MQTT bridge to AWS IoT Core
    // - JWT authentication middleware
    // - CORS and compression middleware

    tracing::info!("zc-cloud-api ready (skeleton — no routes wired yet)");
    Ok(())
}
