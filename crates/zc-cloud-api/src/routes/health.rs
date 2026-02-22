//! Health check endpoint.

use axum::Json;
use serde_json::{Value, json};

/// GET /health — liveness check.
pub async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
