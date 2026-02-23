//! ZeroClaw Cloud API — fleet management REST server.
//!
//! Provides REST endpoints for device registry, command dispatch,
//! telemetry queries, real-time updates via WebSocket, and an optional
//! MQTT bridge to forward commands to devices and ingest responses.

use std::sync::Arc;

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use zc_cloud_api::config::ApiConfig;
use zc_cloud_api::inference::InferenceEngine;
use zc_cloud_api::state::AppState;
use zc_cloud_api::{db, inference, mqtt_bridge, routes};

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
    let mut state = if let Ok(database_url) = std::env::var("DATABASE_URL") {
        tracing::info!("connecting to PostgreSQL");
        let pool = db::connect(&database_url).await?;
        AppState::with_pool(pool, inference)
    } else {
        tracing::warn!("DATABASE_URL not set — using in-memory state with sample data");
        AppState::with_sample_data()
    };

    // Start MQTT bridge if enabled.
    if config.mqtt_enabled {
        if config.mqtt_fleet_id.is_empty() {
            anyhow::bail!("MQTT_ENABLED=true but MQTT_FLEET_ID is not set");
        }

        tracing::info!(
            broker = format!("{}:{}", config.mqtt_broker_host, config.mqtt_broker_port),
            fleet_id = %config.mqtt_fleet_id,
            tls = config.mqtt_use_tls,
            "connecting to mqtt broker"
        );

        let (channel, eventloop) = if config.mqtt_use_tls {
            let mqtt_config = zc_mqtt_channel::MqttConfig {
                broker_host: config.mqtt_broker_host.clone(),
                broker_port: config.mqtt_broker_port,
                client_id: "zc-cloud-api".to_string(),
                ca_cert_path: config
                    .mqtt_ca_cert
                    .clone()
                    .unwrap_or_else(|| "certs/ca.pem".to_string()),
                client_cert_path: config
                    .mqtt_client_cert
                    .clone()
                    .unwrap_or_else(|| "certs/client.pem".to_string()),
                client_key_path: config
                    .mqtt_client_key
                    .clone()
                    .unwrap_or_else(|| "certs/client.key".to_string()),
                keepalive_secs: 30,
            };
            zc_mqtt_channel::MqttChannel::new(&mqtt_config, &config.mqtt_fleet_id, "cloud-api")?
        } else {
            zc_mqtt_channel::MqttChannel::new_plaintext(
                &config.mqtt_broker_host,
                config.mqtt_broker_port,
                "zc-cloud-api",
                &config.mqtt_fleet_id,
                "cloud-api",
            )
        };

        // Subscribe to fleet-wide topics.
        channel
            .subscribe_fleet_responses()
            .await
            .map_err(|e| anyhow::anyhow!("failed to subscribe to fleet responses: {e}"))?;
        channel
            .subscribe_fleet_heartbeats()
            .await
            .map_err(|e| anyhow::anyhow!("failed to subscribe to fleet heartbeats: {e}"))?;
        channel
            .subscribe_fleet_shadow_updates()
            .await
            .map_err(|e| anyhow::anyhow!("failed to subscribe to fleet shadow updates: {e}"))?;
        // Subscribe to all three telemetry sources.
        for source in &["obd2", "system", "canbus"] {
            channel
                .subscribe_fleet_telemetry(source)
                .await
                .map_err(|e| {
                    anyhow::anyhow!("failed to subscribe to fleet telemetry/{source}: {e}")
                })?;
        }

        tracing::info!("mqtt subscriptions established");

        state.mqtt = Some(Arc::new(channel));

        // Spawn the bridge event loop.
        let bridge_state = state.clone();
        tokio::spawn(mqtt_bridge::run(eventloop, bridge_state));

        tracing::info!("mqtt bridge spawned");
    }

    let app = routes::build_router(state);

    let addr = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!(addr = %addr, "listening");

    axum::serve(listener, app).await?;

    Ok(())
}
