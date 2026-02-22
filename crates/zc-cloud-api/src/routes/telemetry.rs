//! Telemetry query and ingestion endpoints.

use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::error::{ApiError, ApiResult};
use crate::events::WsEvent;
use crate::state::AppState;

/// Query parameters for telemetry requests.
#[derive(Debug, Deserialize)]
pub struct TelemetryQuery {
    /// Filter by telemetry source (obd2, system, canbus).
    pub source: Option<String>,
    /// Maximum number of results.
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    100
}

/// Request body for ingesting telemetry readings.
#[derive(Debug, Deserialize)]
pub struct IngestTelemetryRequest {
    pub readings: Vec<TelemetryReadingInput>,
}

/// A single telemetry reading in the ingestion request.
#[derive(Debug, Deserialize)]
pub struct TelemetryReadingInput {
    pub metric_name: String,
    pub value_numeric: Option<f64>,
    pub value_text: Option<String>,
    pub value_json: Option<serde_json::Value>,
    pub unit: Option<String>,
    pub source: String,
    pub time: Option<DateTime<Utc>>,
}

/// GET /api/v1/devices/:id/telemetry — query device telemetry.
pub async fn get_telemetry(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    Query(query): Query<TelemetryQuery>,
) -> ApiResult<Json<serde_json::Value>> {
    // Verify device exists
    if let Some(pool) = &state.pool {
        let exists = crate::db::devices::exists(pool, &device_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        if !exists {
            return Err(ApiError::NotFound(format!(
                "device '{device_id}' not found"
            )));
        }

        // Query real telemetry data
        let rows = crate::db::telemetry::query_readings(
            pool,
            &device_id,
            query.source.as_deref(),
            query.limit,
        )
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

        let readings: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "time": r.time,
                    "metric_name": r.metric_name,
                    "value_numeric": r.value_numeric,
                    "value_text": r.value_text,
                    "value_json": r.value_json,
                    "unit": r.unit,
                    "source": r.source,
                })
            })
            .collect();

        return Ok(Json(serde_json::json!({
            "device_id": device_id,
            "source": query.source,
            "limit": query.limit,
            "readings": readings,
        })));
    }

    // In-memory fallback: verify device exists, return empty readings
    {
        let devices = state.devices.read().await;
        if !devices.contains_key(&device_id) {
            return Err(ApiError::NotFound(format!(
                "device '{device_id}' not found"
            )));
        }
    }

    Ok(Json(serde_json::json!({
        "device_id": device_id,
        "source": query.source,
        "limit": query.limit,
        "readings": [],
        "message": "telemetry storage not yet implemented (in-memory mode)"
    })))
}

/// POST /api/v1/devices/:id/telemetry — ingest telemetry readings.
pub async fn ingest_telemetry(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    Json(req): Json<IngestTelemetryRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let now = Utc::now();
    let count = req.readings.len();

    // Determine dominant source for the event broadcast.
    let source = req
        .readings
        .first()
        .map(|r| r.source.clone())
        .unwrap_or_else(|| "unknown".to_string());

    // Verify device exists
    if let Some(pool) = &state.pool {
        let exists = crate::db::devices::exists(pool, &device_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        if !exists {
            return Err(ApiError::NotFound(format!(
                "device '{device_id}' not found"
            )));
        }

        // Convert to TelemetryRow vec and insert.
        let rows: Vec<crate::db::telemetry::TelemetryRow> = req
            .readings
            .into_iter()
            .map(|r| crate::db::telemetry::TelemetryRow {
                time: r.time.unwrap_or(now),
                device_id: device_id.clone(),
                metric_name: r.metric_name,
                value_numeric: r.value_numeric,
                value_text: r.value_text,
                value_json: r.value_json,
                unit: r.unit,
                source: r.source,
            })
            .collect();
        crate::db::telemetry::insert_batch(pool, &rows)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
    } else {
        // In-memory fallback: verify device exists (accept data loss).
        let devices = state.devices.read().await;
        if !devices.contains_key(&device_id) {
            return Err(ApiError::NotFound(format!(
                "device '{device_id}' not found"
            )));
        }
    }

    tracing::debug!(device_id = %device_id, count = count, "telemetry ingested");

    let _ = state.event_tx.send(WsEvent::TelemetryIngested {
        device_id,
        count,
        source,
        timestamp: now,
    });

    Ok(Json(serde_json::json!({
        "status": "ok",
        "count": count,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::build_router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn app() -> axum::Router {
        build_router(AppState::with_sample_data())
    }

    #[tokio::test]
    async fn ingest_single_reading() {
        let body = serde_json::json!({
            "readings": [{
                "metric_name": "engine_rpm",
                "value_numeric": 3500.0,
                "unit": "rpm",
                "source": "obd2"
            }]
        });

        let response = app()
            .oneshot(
                Request::post("/api/v1/devices/rpi-001/telemetry")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["count"], 1);
    }

    #[tokio::test]
    async fn ingest_batch_readings() {
        let body = serde_json::json!({
            "readings": [
                { "metric_name": "engine_rpm", "value_numeric": 3500.0, "source": "obd2" },
                { "metric_name": "coolant_temp", "value_numeric": 90.0, "unit": "celsius", "source": "obd2" },
                { "metric_name": "cpu_usage", "value_numeric": 45.2, "unit": "percent", "source": "system" }
            ]
        });

        let response = app()
            .oneshot(
                Request::post("/api/v1/devices/rpi-001/telemetry")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["count"], 3);
    }

    #[tokio::test]
    async fn ingest_telemetry_unknown_device() {
        let body = serde_json::json!({
            "readings": [{
                "metric_name": "engine_rpm",
                "value_numeric": 3500.0,
                "source": "obd2"
            }]
        });

        let response = app()
            .oneshot(
                Request::post("/api/v1/devices/nonexistent/telemetry")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn ingest_telemetry_broadcasts_event() {
        let state = AppState::with_sample_data();
        let mut rx = state.event_tx.subscribe();
        let app = build_router(state);

        let body = serde_json::json!({
            "readings": [{
                "metric_name": "engine_rpm",
                "value_numeric": 3500.0,
                "source": "obd2"
            }]
        });

        app.oneshot(
            Request::post("/api/v1/devices/rpi-001/telemetry")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

        let event = rx.try_recv().unwrap();
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("telemetry_ingested"));
        assert!(json.contains("rpi-001"));
    }
}
