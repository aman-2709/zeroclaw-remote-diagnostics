//! API route definitions and router builder.

pub mod commands;
pub mod devices;
pub mod health;
pub mod heartbeat;
pub mod responses;
pub mod telemetry;
pub mod ws;

use axum::Router;
use axum::routing::{get, post};
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::state::AppState;

/// Build the Axum router with all routes and middleware.
pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api = Router::new()
        // Device endpoints
        .route(
            "/devices",
            get(devices::list_devices).post(devices::provision_device),
        )
        .route("/devices/{id}", get(devices::get_device))
        // Command endpoints
        .route(
            "/commands",
            get(commands::list_commands).post(commands::send_command),
        )
        .route("/commands/{id}", get(commands::get_command))
        // Command response ingestion
        .route("/commands/{id}/respond", post(responses::ingest_response))
        // Telemetry endpoints
        .route(
            "/devices/{id}/telemetry",
            get(telemetry::get_telemetry).post(telemetry::ingest_telemetry),
        )
        // Heartbeat ingestion
        .route("/heartbeat", post(heartbeat::ingest_heartbeat))
        // WebSocket endpoint
        .route("/ws", get(ws::ws_handler));

    Router::new()
        .route("/health", get(health::health))
        .nest("/api/v1", api)
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(cors)
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn app() -> Router {
        build_router(AppState::with_sample_data())
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let response = app()
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn list_devices() {
        let response = app()
            .oneshot(Request::get("/api/v1/devices").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.len(), 3);
    }

    #[tokio::test]
    async fn get_device_found() {
        let response = app()
            .oneshot(
                Request::get("/api/v1/devices/rpi-001")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["device_id"], "rpi-001");
    }

    #[tokio::test]
    async fn get_device_not_found() {
        let response = app()
            .oneshot(
                Request::get("/api/v1/devices/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn send_command_to_known_device() {
        let body = serde_json::json!({
            "device_id": "rpi-001",
            "fleet_id": "fleet-alpha",
            "command": "read DTCs",
            "initiated_by": "admin@test.com"
        });

        let response = app()
            .oneshot(
                Request::post("/api/v1/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["device_id"], "rpi-001");
        assert!(json["id"].is_string());
    }

    #[tokio::test]
    async fn send_command_to_unknown_device() {
        let body = serde_json::json!({
            "device_id": "ghost-999",
            "fleet_id": "fleet-alpha",
            "command": "hello",
            "initiated_by": "admin"
        });

        let response = app()
            .oneshot(
                Request::post("/api/v1/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn list_commands_empty() {
        let response = app()
            .oneshot(
                Request::get("/api/v1/commands")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert!(json.is_empty());
    }

    #[tokio::test]
    async fn telemetry_for_known_device() {
        let response = app()
            .oneshot(
                Request::get("/api/v1/devices/rpi-001/telemetry")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["device_id"], "rpi-001");
        assert!(json["readings"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn telemetry_for_unknown_device() {
        let response = app()
            .oneshot(
                Request::get("/api/v1/devices/nonexistent/telemetry")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
