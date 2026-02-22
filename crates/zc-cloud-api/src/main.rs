//! ZeroClaw Cloud API — fleet management REST server.
//!
//! Provides REST endpoints for device registry, command dispatch,
//! telemetry queries, and real-time updates via WebSocket (Phase 2).

mod config;
pub mod db;
mod error;
pub mod events;
pub mod inference;
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

    let config = ApiConfig::default();

    // Connect to PostgreSQL if DATABASE_URL is set, otherwise use in-memory state.
    let state = if let Ok(database_url) = std::env::var("DATABASE_URL") {
        tracing::info!("connecting to PostgreSQL");
        let pool = db::connect(&database_url).await?;
        AppState::with_pool(pool)
    } else {
        tracing::warn!("DATABASE_URL not set — using in-memory state with sample data");
        AppState::with_sample_data()
    };

    let app = routes::build_router(state);

    let addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!(addr = %addr, "listening");

    axum::serve(listener, app).await?;

    Ok(())
}
