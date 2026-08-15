//! Subscription notification join integration tests (NGSI-LD 5.2.14.1).
//!
//! The cases are grouped by what they pin down: how far the traversal reaches
//! (`depth`), and the order the resolved entities are delivered in (`order`).
//! This module owns only the helpers those groups share.
//!
//! Naming convention: `notification_join_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod concurrency;
mod depth;
mod order;

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::sleep;

async fn create(client: &TestClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED);
}

async fn append_attrs(client: &TestClient, entity_id: &str, fragment: &Value) {
    let response = client.append_entity_attrs(entity_id, fragment).await.expect("append_entity_attrs failed");
    assert_eq!(response.status, StatusCode::NO_CONTENT);
}

async fn create_subscription(client: &TestClient, body: &Value) {
    let response = client.create_subscription(body).await.expect("create_subscription failed");
    assert_eq!(response.status, StatusCode::CREATED);
    sleep(Duration::from_millis(150)).await;
}

fn subscription_with_flat_join(subscription_id: &str, entity_id: &str, endpoint_url: &str, join_level: Option<u32>) -> Value {
    let mut notification = json!({
        "endpoint": {"uri": endpoint_url, "accept": "application/json"},
        "join": "flat"
    });
    if let Some(level) = join_level {
        notification["joinLevel"] = json!(level);
    }
    json!({
        "id": subscription_id,
        "type": "Subscription",
        "entities": [{"type": "Building", "id": entity_id}],
        "notification": notification,
        "@context": [CORE_CONTEXT]
    })
}

/// An attribute append that makes the subscription fire.
fn trigger_fragment(name: &str) -> Value {
    json!({"name": {"type": "Property", "value": name}, "@context": [CORE_CONTEXT]})
}
