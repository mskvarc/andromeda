//! Subscriptions (clause 5.8) are notified of the Core writes history leaves
//! out: a notification reports a change of current state, which the write
//! still makes.

use crate::{
    evolution::{add, evolution, expected, observations, seed},
    observation::{core_context, fragment, reading},
};
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

/// The instances the test seeds: `v` = 1 at minute 0 and 2 at minute 1.
const SEEDED: [(i64, u32); 2] = [(1, 0), (2, 1)];

/// How long a test waits for one notification.
const NOTIFICATION_DEADLINE: Duration = Duration::from_secs(15);

/// The value of `v` the next notification reports for `id`.
async fn next_notified_value(receiver: &NotificationServer, id: &str) -> Value {
    let request = receiver.next_request(NOTIFICATION_DEADLINE).await.expect("a notification arrives");
    let notification: Value = request.json().expect("a JSON notification");
    let data = notification["data"].as_array().expect("clause 5.3.1: a data array");
    let notified = data.iter().find(|entity| entity["id"] == json!(id)).expect("the entity is notified");
    notified["v"]["value"].clone() // clone: the caller owns the reported value.
}

#[tokio::test]
async fn unrecorded_core_writes_and_temporal_writes_still_notify() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("mock server configuration"))
        .await
        .expect("the receiver binds");
    let entity_type = scope.entity_type("Sensor");
    let id = scope.id("Sensor:notified");
    seed(scope.client(), &id, &entity_type, &SEEDED).await;

    let subscription = json!({
        "id": scope.id("Subscription:notified"),
        "type": "Subscription",
        "entities": [{"type": entity_type}],
        "watchedAttributes": ["v"],
        "notification": {"format": "normalized", "endpoint": {"uri": receiver.endpoint_url().expect("an endpoint"), "accept": "application/json"}},
        "@context": core_context()
    });
    let response = scope.client().create_subscription(&subscription).await.expect("the broker answers");
    assert_eq!(response.status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&response.body));

    let response = scope
        .client()
        .update_entity_attrs(&id, &fragment(json!({"v": reading(5, 2)})))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert_eq!(next_notified_value(&receiver, &id).await, json!(5));

    add(scope.client(), &id, json!({"v": reading(8, 10)})).await;
    assert_eq!(
        next_notified_value(&receiver, &id).await,
        json!(8),
        "a newer Temporal API instance changes current state"
    );

    let recorded = evolution(scope.client(), &id).await.expect("the evolution exists");
    assert_eq!(observations(&recorded, "v"), expected(&[(1, 0), (2, 1), (8, 10)]));

    receiver.shutdown().await.expect("the receiver stops");
    scope.finish().await.expect("cleanup failed");
}
