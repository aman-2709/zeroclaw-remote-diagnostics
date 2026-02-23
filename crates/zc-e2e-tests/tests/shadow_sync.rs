//! End-to-end tests for device shadow sync across cloud API and fleet agent.

mod helpers;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use helpers::TestHarness;
use http_body_util::BodyExt;
use tower::ServiceExt;
use zc_protocol::shadows::ShadowUpdate;
use zc_protocol::topics;

#[tokio::test]
async fn e2e_shadow_report_stored_and_queryable() {
    let harness = TestHarness::with_sample_data();

    // Device reports shadow update via MQTT bridge.
    let update = ShadowUpdate {
        device_id: "rpi-001".into(),
        shadow_name: "diagnostics".into(),
        reported: serde_json::json!({
            "firmware": "0.1.0",
            "tool_count": 10,
            "uptime_secs": 300
        }),
        version: 1,
    };

    let payload = serde_json::to_vec(&update).unwrap();
    let topic = topics::shadow_update("fleet-alpha", "rpi-001");
    zc_cloud_api::mqtt_bridge::handle_incoming(&topic, &payload, &harness.cloud_state).await;

    // Query shadow via REST.
    let response = harness
        .cloud_router
        .clone()
        .oneshot(
            Request::get("/api/v1/devices/rpi-001/shadows/diagnostics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["reported"]["firmware"], "0.1.0");
    assert_eq!(json["reported"]["tool_count"], 10);
    assert_eq!(json["shadow_name"], "diagnostics");
}

#[tokio::test]
async fn e2e_set_desired_publishes_delta() {
    let harness = TestHarness::with_sample_data();

    // Pre-populate reported state via MQTT bridge.
    let update = ShadowUpdate {
        device_id: "rpi-001".into(),
        shadow_name: "config".into(),
        reported: serde_json::json!({"firmware": "0.1.0"}),
        version: 1,
    };
    let payload = serde_json::to_vec(&update).unwrap();
    let topic = topics::shadow_update("fleet-alpha", "rpi-001");
    zc_cloud_api::mqtt_bridge::handle_incoming(&topic, &payload, &harness.cloud_state).await;

    // Clear MQTT mock to isolate delta publish.
    harness.mqtt.reset();

    // Set desired via REST.
    let body = serde_json::json!({"desired": {"firmware": "0.2.0"}});
    let response = harness
        .cloud_router
        .clone()
        .oneshot(
            Request::put("/api/v1/devices/rpi-001/shadows/config/desired")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Verify delta was published to MQTT.
    let delta_msgs = harness
        .mqtt
        .published_to("fleet/fleet-alpha/rpi-001/shadow/delta");
    assert_eq!(delta_msgs.len(), 1);
    let delta: zc_protocol::shadows::ShadowDelta =
        serde_json::from_slice(&delta_msgs[0].payload).unwrap();
    assert_eq!(delta.delta["firmware"], "0.2.0");
}

#[tokio::test]
async fn e2e_shadow_round_trip_delta_resolves() {
    let harness = TestHarness::with_sample_data();

    // Set desired state.
    let body = serde_json::json!({"desired": {"firmware": "0.2.0"}});
    let _response = harness
        .cloud_router
        .clone()
        .oneshot(
            Request::put("/api/v1/devices/rpi-001/shadows/config/desired")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Clear MQTT mock.
    harness.mqtt.reset();

    // Device reports matching state.
    let update = ShadowUpdate {
        device_id: "rpi-001".into(),
        shadow_name: "config".into(),
        reported: serde_json::json!({"firmware": "0.2.0"}),
        version: 2,
    };
    let payload = serde_json::to_vec(&update).unwrap();
    let topic = topics::shadow_update("fleet-alpha", "rpi-001");
    zc_cloud_api::mqtt_bridge::handle_incoming(&topic, &payload, &harness.cloud_state).await;

    // No delta should be published since reported matches desired.
    let delta_msgs = harness
        .mqtt
        .published_to("fleet/fleet-alpha/rpi-001/shadow/delta");
    assert!(
        delta_msgs.is_empty(),
        "no delta expected when reported matches desired"
    );
}

#[tokio::test]
async fn e2e_shadow_update_broadcasts_ws_event() {
    let harness = TestHarness::with_sample_data();
    let mut event_rx = harness.cloud_state.event_tx.subscribe();

    let update = ShadowUpdate {
        device_id: "rpi-001".into(),
        shadow_name: "diagnostics".into(),
        reported: serde_json::json!({"uptime": 600}),
        version: 1,
    };

    let payload = serde_json::to_vec(&update).unwrap();
    let topic = topics::shadow_update("fleet-alpha", "rpi-001");
    zc_cloud_api::mqtt_bridge::handle_incoming(&topic, &payload, &harness.cloud_state).await;

    let event = event_rx.try_recv().unwrap();
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains(r#""type":"shadow_updated""#));
    assert!(json.contains(r#""device_id":"rpi-001""#));
    assert!(json.contains(r#""shadow_name":"diagnostics""#));
}

#[tokio::test]
async fn e2e_list_shadows_returns_all() {
    let harness = TestHarness::with_sample_data();

    // Report two different named shadows.
    for (name, data) in &[
        ("diagnostics", serde_json::json!({"firmware": "0.1.0"})),
        ("config", serde_json::json!({"mode": "normal"})),
    ] {
        let update = ShadowUpdate {
            device_id: "rpi-001".into(),
            shadow_name: name.to_string(),
            reported: data.clone(),
            version: 1,
        };
        let payload = serde_json::to_vec(&update).unwrap();
        let topic = topics::shadow_update("fleet-alpha", "rpi-001");
        zc_cloud_api::mqtt_bridge::handle_incoming(&topic, &payload, &harness.cloud_state).await;
    }

    // List shadows via REST.
    let response = harness
        .cloud_router
        .clone()
        .oneshot(
            Request::get("/api/v1/devices/rpi-001/shadows")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json.len(), 2);

    let names: Vec<&str> = json
        .iter()
        .map(|s| s["shadow_name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"diagnostics"));
    assert!(names.contains(&"config"));
}
