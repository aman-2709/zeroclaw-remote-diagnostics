//! API route definitions and router builder.

pub mod commands;
pub mod devices;
pub mod health;
pub mod heartbeat;
pub mod responses;
pub mod shadows;
pub mod telemetry;
pub mod ws;

use std::sync::Arc;

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use subtle::ConstantTimeEq;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

use crate::state::AppState;

/// Security and request handling settings applied to the API router.
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// Optional bearer token protecting all `/api/v1` routes.
    pub api_auth_token: Option<String>,
    /// Allowed browser origins. Empty means permissive local-development mode.
    pub cors_origins: Vec<String>,
    /// Maximum accepted request body size.
    pub max_body_bytes: usize,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            api_auth_token: None,
            cors_origins: vec![],
            max_body_bytes: 1024 * 1024,
        }
    }
}

/// Build the Axum router with development-compatible defaults.
///
/// The production binary passes an explicit `RouterConfig`; keeping this
/// wrapper preserves the lightweight unauthenticated router used by unit tests.
pub fn build_router(state: AppState) -> Router {
    build_router_with_config(state, RouterConfig::default())
}

/// Build the Axum router with production security and request limits.
pub fn build_router_with_config(state: AppState, config: RouterConfig) -> Router {
    let cors = if config.cors_origins.is_empty() {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::OPTIONS])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
    } else {
        let origins = config
            .cors_origins
            .iter()
            .filter_map(|origin| match origin.parse::<HeaderValue>() {
                Ok(value) => Some(value),
                Err(error) => {
                    tracing::warn!(origin, %error, "ignoring invalid CORS origin");
                    None
                }
            })
            .collect::<Vec<_>>();
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::OPTIONS])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
    };

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
        // Shadow endpoints
        .route("/devices/{id}/shadows", get(shadows::list_shadows))
        .route("/devices/{id}/shadows/{name}", get(shadows::get_shadow))
        .route(
            "/devices/{id}/shadows/{name}/desired",
            put(shadows::set_desired),
        )
        // Heartbeat ingestion
        .route("/heartbeat", post(heartbeat::ingest_heartbeat))
        // WebSocket endpoint
        .route("/ws", get(ws::ws_handler))
        .layer(RequestBodyLimitLayer::new(config.max_body_bytes));

    let api = if let Some(token) = config.api_auth_token {
        let expected = Arc::<[u8]>::from(token.into_bytes());
        api.layer(middleware::from_fn(move |request: Request, next: Next| {
            let expected = Arc::clone(&expected);
            async move {
                let authorized = request
                    .headers()
                    .get(header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.strip_prefix("Bearer "))
                    .map(|provided| provided.as_bytes().ct_eq(expected.as_ref()).into())
                    .unwrap_or(false);

                if !authorized {
                    return (
                        StatusCode::UNAUTHORIZED,
                        [(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"))],
                        axum::Json(serde_json::json!({
                            "error": "missing or invalid bearer token",
                            "status": StatusCode::UNAUTHORIZED.as_u16(),
                        })),
                    )
                        .into_response();
                }

                next.run(request).await
            }
        }))
    } else {
        api
    };

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

    #[tokio::test]
    async fn protected_api_rejects_missing_and_invalid_bearer_tokens() {
        let security = RouterConfig {
            api_auth_token: Some("test-token-with-at-least-32-characters".into()),
            cors_origins: vec![],
            max_body_bytes: 1024,
        };
        let protected = build_router_with_config(AppState::with_sample_data(), security.clone());

        let missing = protected
            .clone()
            .oneshot(Request::get("/api/v1/devices").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

        let invalid = protected
            .clone()
            .oneshot(
                Request::get("/api/v1/devices")
                    .header("authorization", "Bearer wrong-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);

        let valid = protected
            .oneshot(
                Request::get("/api/v1/devices")
                    .header(
                        "authorization",
                        "Bearer test-token-with-at-least-32-characters",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(valid.status(), StatusCode::OK);

        let health = build_router_with_config(AppState::with_sample_data(), security)
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn request_body_limit_returns_payload_too_large() {
        let app = build_router_with_config(
            AppState::with_sample_data(),
            RouterConfig {
                max_body_bytes: 32,
                ..RouterConfig::default()
            },
        );
        let response = app
            .oneshot(
                Request::post("/api/v1/commands")
                    .header("content-type", "application/json")
                    .body(Body::from(vec![b'x'; 128]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
