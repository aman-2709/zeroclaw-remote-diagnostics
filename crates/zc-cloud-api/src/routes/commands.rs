//! Command dispatch endpoints.

use axum::Json;
use axum::extract::{Path, State};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, CommandRecord};
use zc_protocol::commands::CommandEnvelope;

/// Request body for dispatching a command.
#[derive(Debug, Deserialize)]
pub struct SendCommandRequest {
    /// Target device ID.
    pub device_id: String,
    /// Target fleet ID.
    pub fleet_id: String,
    /// Natural-language command text.
    pub command: String,
    /// Who is sending this command.
    pub initiated_by: String,
}

/// POST /api/v1/commands — dispatch a command to a device.
pub async fn send_command(
    State(state): State<AppState>,
    Json(req): Json<SendCommandRequest>,
) -> ApiResult<Json<CommandEnvelope>> {
    // Verify device exists
    {
        let devices = state.devices.read().await;
        if !devices.contains_key(&req.device_id) {
            return Err(ApiError::NotFound(format!(
                "device '{}' not found",
                req.device_id
            )));
        }
    }

    let envelope = CommandEnvelope::new(
        &req.fleet_id,
        &req.device_id,
        &req.command,
        &req.initiated_by,
    );

    // Store the command record
    {
        let mut commands = state.commands.write().await;
        commands.push(CommandRecord {
            envelope: envelope.clone(),
            response: None,
            created_at: Utc::now(),
        });
    }

    // Phase 2: Publish to MQTT via IoT Core data plane
    tracing::info!(
        command_id = %envelope.id,
        device_id = %req.device_id,
        "command dispatched (MQTT publish not yet wired)"
    );

    Ok(Json(envelope))
}

/// GET /api/v1/commands/:id — get command status.
pub async fn get_command(
    State(state): State<AppState>,
    Path(command_id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    let commands = state.commands.read().await;
    let record = commands
        .iter()
        .find(|r| r.envelope.id == command_id)
        .ok_or_else(|| ApiError::NotFound(format!("command '{command_id}' not found")))?;

    let json = serde_json::json!({
        "command": record.envelope,
        "response": record.response,
        "created_at": record.created_at,
    });
    Ok(Json(json))
}

/// GET /api/v1/commands — list recent commands.
pub async fn list_commands(State(state): State<AppState>) -> Json<Vec<serde_json::Value>> {
    let commands = state.commands.read().await;
    let recent: Vec<serde_json::Value> = commands
        .iter()
        .rev()
        .take(50)
        .map(|r| {
            serde_json::json!({
                "id": r.envelope.id,
                "device_id": r.envelope.device_id,
                "command": r.envelope.natural_language,
                "status": r.response.as_ref().map(|r| &r.status),
                "created_at": r.created_at,
            })
        })
        .collect();
    Json(recent)
}
