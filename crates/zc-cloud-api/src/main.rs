//! ZeroClaw Cloud API — fleet management REST server.
//!
//! Provides REST endpoints for device registry, command dispatch,
//! telemetry queries, and real-time updates via WebSocket (Phase 2).

mod config;
mod error;
mod routes;
mod state;

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use crate::config::ApiConfig;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .json()
        .init();

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "zc-cloud-api starting");

    // Phase 2: Load config from file/env. For now, use defaults.
    let config = ApiConfig::default();

    // Phase 2: Replace with PostgreSQL-backed state.
    let state = AppState::with_sample_data();

    let app = routes::build_router(state);

    let addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!(addr = %addr, "listening");

    axum::serve(listener, app).await?;

    Ok(())
}
