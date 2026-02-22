//! Command dispatch endpoints.

use axum::Json;
use axum::extract::{Path, State};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::events::WsEvent;
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
    if let Some(pool) = &state.pool {
        let exists = crate::db::devices::exists(pool, &req.device_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        if !exists {
            return Err(ApiError::NotFound(format!(
                "device '{}' not found",
                req.device_id
            )));
        }
    } else {
        let devices = state.devices.read().await;
        if !devices.contains_key(&req.device_id) {
            return Err(ApiError::NotFound(format!(
                "device '{}' not found",
                req.device_id
            )));
        }
    }

    let mut envelope = CommandEnvelope::new(
        &req.fleet_id,
        &req.device_id,
        &req.command,
        &req.initiated_by,
    );

    // Run NL inference to parse command into tool invocation.
    let parse_result = state.inference.parse(&req.command).await;
    let (parsed_intent, inference_tier) = match &parse_result {
        Some(r) => (Some(r.intent.clone()), Some(r.tier.clone())),
        None => (None, None),
    };
    envelope.parsed_intent = parsed_intent.clone();

    // Store the command (with parsed intent if available)
    if let Some(pool) = &state.pool {
        let row = crate::db::commands::CommandRow {
            id: envelope.id,
            fleet_id: envelope.fleet_id.clone(),
            device_id: envelope.device_id.clone(),
            natural_language: envelope.natural_language.clone(),
            initiated_by: envelope.initiated_by.clone(),
            correlation_id: envelope.correlation_id,
            timeout_secs: envelope.timeout_secs as i32,
            tool_name: parsed_intent.as_ref().map(|i| i.tool_name.clone()),
            tool_args: parsed_intent.as_ref().map(|i| i.tool_args.clone()),
            confidence: parsed_intent.as_ref().map(|i| i.confidence),
            status: "pending".to_string(),
            inference_tier,
            response_text: None,
            response_data: None,
            latency_ms: None,
            responded_at: None,
            error: None,
            created_at: envelope.created_at,
        };
        crate::db::commands::insert(pool, &row)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
    } else {
        let mut commands = state.commands.write().await;
        commands.push(CommandRecord {
            envelope: envelope.clone(),
            response: None,
            created_at: Utc::now(),
        });
    }

    tracing::info!(
        command_id = %envelope.id,
        device_id = %req.device_id,
        "command dispatched"
    );

    // Broadcast real-time event (ignore error if no receivers).
    let _ = state.event_tx.send(WsEvent::CommandDispatched {
        command_id: envelope.id,
        device_id: envelope.device_id.clone(),
        command: envelope.natural_language.clone(),
        initiated_by: envelope.initiated_by.clone(),
        created_at: envelope.created_at,
    });

    // Publish command envelope to MQTT if the bridge is connected.
    if let Some(mqtt) = &state.mqtt {
        let topic = zc_protocol::topics::command_request(&envelope.fleet_id, &envelope.device_id);
        if let Err(e) = mqtt
            .publish(
                &topic,
                &serde_json::to_vec(&envelope).unwrap_or_default(),
                rumqttc::QoS::AtLeastOnce,
            )
            .await
        {
            tracing::error!(error = %e, "failed to publish command to mqtt");
        }
    }

    Ok(Json(envelope))
}

/// GET /api/v1/commands/:id — get command status.
pub async fn get_command(
    State(state): State<AppState>,
    Path(command_id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    if let Some(pool) = &state.pool {
        let row = crate::db::commands::get_by_id(pool, command_id)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .ok_or_else(|| ApiError::NotFound(format!("command '{command_id}' not found")))?;
        let json = serde_json::json!({
            "id": row.id,
            "device_id": row.device_id,
            "command": row.natural_language,
            "status": row.status,
            "tool_name": row.tool_name,
            "tool_args": row.tool_args,
            "confidence": row.confidence,
            "inference_tier": row.inference_tier,
            "response_text": row.response_text,
            "response_data": row.response_data,
            "latency_ms": row.latency_ms,
            "error": row.error,
            "created_at": row.created_at,
            "responded_at": row.responded_at,
        });
        return Ok(Json(json));
    }

    // In-memory fallback
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
pub async fn list_commands(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<serde_json::Value>>> {
    if let Some(pool) = &state.pool {
        let rows = crate::db::commands::list_recent(pool, 50)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        let recent: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "device_id": r.device_id,
                    "command": r.natural_language,
                    "status": r.status,
                    "created_at": r.created_at,
                })
            })
            .collect();
        return Ok(Json(recent));
    }

    // In-memory fallback
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
    Ok(Json(recent))
}
