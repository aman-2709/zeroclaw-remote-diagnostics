//! Command response ingestion endpoint.

use axum::Json;
use axum::extract::{Path, State};
use chrono::Utc;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::events::WsEvent;
use crate::state::AppState;
use zc_protocol::commands::CommandResponse;

/// POST /api/v1/commands/{id}/respond — ingest a command response from a device.
pub async fn ingest_response(
    State(state): State<AppState>,
    Path(command_id): Path<Uuid>,
    Json(resp): Json<CommandResponse>,
) -> ApiResult<Json<serde_json::Value>> {
    // Validate that the response matches the path parameter.
    if resp.command_id != command_id {
        return Err(ApiError::BadRequest(format!(
            "command_id mismatch: path={command_id}, body={}",
            resp.command_id
        )));
    }

    let status_str = serde_json::to_value(resp.status)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| format!("{:?}", resp.status).to_lowercase());

    let inference_tier_str = serde_json::to_value(resp.inference_tier)
        .ok()
        .and_then(|v| v.as_str().map(String::from));

    if let Some(pool) = &state.pool {
        // Verify command exists in DB.
        let row = crate::db::commands::get_by_id(pool, command_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound(format!("command '{command_id}' not found")))?;

        // Compute latency from dispatch to response.
        let latency_ms = (resp.responded_at - row.created_at).num_milliseconds();

        crate::db::commands::update_response(
            pool,
            command_id,
            &status_str,
            inference_tier_str.as_deref().unwrap_or("unknown"),
            resp.response_text.as_deref(),
            resp.response_data.as_ref(),
            latency_ms,
            resp.error.as_deref(),
        )
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    } else {
        // In-memory mode: find and update the command record.
        let mut commands = state.commands.write().await;
        let record = commands
            .iter_mut()
            .find(|r| r.envelope.id == command_id)
            .ok_or_else(|| ApiError::NotFound(format!("command '{command_id}' not found")))?;
        record.response = Some(resp.clone());
    }

    tracing::info!(command_id = %command_id, status = %status_str, "command response ingested");

    // Broadcast real-time event.
    let _ = state.event_tx.send(WsEvent::CommandResponse {
        command_id,
        device_id: resp.device_id.clone(),
        status: status_str,
        inference_tier: inference_tier_str,
        response_text: resp.response_text.clone(),
        latency_ms: Some(resp.latency_ms as i64),
        responded_at: Utc::now(),
    });

    Ok(Json(serde_json::json!({ "status": "ok" })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::build_router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use chrono::Utc;
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use zc_protocol::commands::{CommandStatus, InferenceTier};

    fn app_with_command() -> (axum::Router, Uuid, AppState) {
        let state = AppState::with_sample_data();
        let cmd_id = Uuid::now_v7();

        // Pre-populate a pending command in in-memory state.
        let envelope = zc_protocol::commands::CommandEnvelope {
            id: cmd_id,
            fleet_id: "fleet-alpha".into(),
            device_id: "rpi-001".into(),
            natural_language: "read DTCs".into(),
            parsed_intent: None,
            correlation_id: cmd_id,
            initiated_by: "admin".into(),
            created_at: Utc::now(),
            timeout_secs: 30,
        };

        // We need to block to insert — use a sync approach via the Arc.
        let commands = state.commands.clone();
        // Use try_write since we're in a sync context during setup.
        let mut guard = commands.try_write().unwrap();
        guard.push(crate::state::CommandRecord {
            envelope,
            response: None,
            created_at: Utc::now(),
        });
        drop(guard);

        let app = build_router(state.clone());
        (app, cmd_id, state)
    }

    #[tokio::test]
    async fn ingest_valid_response() {
        let (app, cmd_id, state) = app_with_command();

        let resp = CommandResponse {
            command_id: cmd_id,
            correlation_id: cmd_id,
            device_id: "rpi-001".into(),
            status: CommandStatus::Completed,
            inference_tier: InferenceTier::Local,
            response_text: Some("No DTCs found".into()),
            response_data: None,
            latency_ms: 42,
            responded_at: Utc::now(),
            error: None,
        };

        let response = app
            .oneshot(
                Request::post(format!("/api/v1/commands/{cmd_id}/respond"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&resp).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");

        // Verify in-memory record was updated.
        let commands = state.commands.read().await;
        let record = commands.iter().find(|r| r.envelope.id == cmd_id).unwrap();
        assert!(record.response.is_some());
        assert_eq!(
            record.response.as_ref().unwrap().status,
            CommandStatus::Completed
        );
    }

    #[tokio::test]
    async fn ingest_response_unknown_command() {
        let state = AppState::with_sample_data();
        let app = build_router(state);
        let fake_id = Uuid::now_v7();

        let resp = CommandResponse {
            command_id: fake_id,
            correlation_id: fake_id,
            device_id: "rpi-001".into(),
            status: CommandStatus::Completed,
            inference_tier: InferenceTier::Local,
            response_text: None,
            response_data: None,
            latency_ms: 10,
            responded_at: Utc::now(),
            error: None,
        };

        let response = app
            .oneshot(
                Request::post(format!("/api/v1/commands/{fake_id}/respond"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&resp).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn ingest_response_broadcasts_event() {
        let (_, cmd_id, state) = app_with_command();
        let mut rx = state.event_tx.subscribe();
        let app = build_router(state);

        let resp = CommandResponse {
            command_id: cmd_id,
            correlation_id: cmd_id,
            device_id: "rpi-001".into(),
            status: CommandStatus::Completed,
            inference_tier: InferenceTier::Local,
            response_text: Some("Engine RPM: 850".into()),
            response_data: None,
            latency_ms: 55,
            responded_at: Utc::now(),
            error: None,
        };

        app.oneshot(
            Request::post(format!("/api/v1/commands/{cmd_id}/respond"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&resp).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

        let event = rx.try_recv().unwrap();
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("command_response"));
        assert!(json.contains("rpi-001"));
        assert!(json.contains("Engine RPM: 850"));
    }

    #[tokio::test]
    async fn ingest_response_id_mismatch() {
        let (app, cmd_id, _) = app_with_command();
        let wrong_id = Uuid::now_v7();

        let resp = CommandResponse {
            command_id: wrong_id,
            correlation_id: wrong_id,
            device_id: "rpi-001".into(),
            status: CommandStatus::Completed,
            inference_tier: InferenceTier::Local,
            response_text: None,
            response_data: None,
            latency_ms: 10,
            responded_at: Utc::now(),
            error: None,
        };

        let response = app
            .oneshot(
                Request::post(format!("/api/v1/commands/{cmd_id}/respond"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&resp).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
