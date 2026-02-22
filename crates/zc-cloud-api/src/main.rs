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

use std::sync::Arc;

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use crate::config::ApiConfig;
use crate::inference::InferenceEngine;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .json()
        .init();

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "zc-cloud-api starting");

    let config = ApiConfig::from_env();

    // Build the inference engine (rule-based, or tiered with Bedrock fallback).
    let inference: Arc<dyn InferenceEngine> = if config.bedrock_enabled {
        tracing::info!("bedrock inference enabled — building tiered engine");
        let bedrock_config = inference::bedrock::BedrockConfig::from_env();
        tracing::info!(model_id = %bedrock_config.model_id, "bedrock model configured");
        let aws_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let bedrock_client = aws_sdk_bedrockruntime::Client::new(&aws_config);
        let bedrock_engine = inference::bedrock::BedrockEngine::new(bedrock_client, bedrock_config);
        let tiered = inference::tiered::TieredEngine::new(
            Box::new(inference::RuleBasedEngine::new()),
            Box::new(bedrock_engine),
        );
        Arc::new(tiered)
    } else {
        tracing::info!("bedrock inference disabled — using rule-based engine only");
        Arc::new(inference::RuleBasedEngine::new())
    };

    // Connect to PostgreSQL if DATABASE_URL is set, otherwise use in-memory state.
    let state = if let Ok(database_url) = std::env::var("DATABASE_URL") {
        tracing::info!("connecting to PostgreSQL");
        let pool = db::connect(&database_url).await?;
        AppState::with_pool(pool, inference)
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
