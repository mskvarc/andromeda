//! Public-API fixtures shared by the focused Snapshot acceptance modules.

use andromeda_test_harness::{configuration::RUNTIME_BROKER_BASE as BASE_URL, serialized_environment::SerializedEnvironment};
use reqwest::{Client, StatusCode, header::HeaderMap};
use serde_json::Value;
use std::time::Duration;

pub(crate) fn client() -> Client {
    Client::new()
}

pub(crate) fn broker_url(path: &str) -> String {
    format!("{BASE_URL}{path}")
}

pub(crate) async fn clean_database() -> SerializedEnvironment {
    SerializedEnvironment::from_environment_reset()
        .await
        .expect("serialized test database environment")
}

pub(crate) async fn create_snapshot_raw(client: &Client, body: &Value) -> (StatusCode, HeaderMap, Value) {
    let response = client
        .post(format!("{BASE_URL}/snapshots"))
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .expect("create Snapshot request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json().await.unwrap_or_default();
    (status, headers, body)
}

pub(crate) async fn create_entity(client: &Client, body: &Value) -> StatusCode {
    client
        .post(broker_url("/entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create Entity request")
        .status()
}

pub(crate) async fn create_entities_batch(client: &Client, entities: &[Value]) -> (StatusCode, Value) {
    let response = client
        .post(broker_url("/entityOperations/create"))
        .header("Content-Type", "application/ld+json")
        .json(entities)
        .send()
        .await
        .expect("batch create Entities request");
    let status = response.status();
    let body = response.json().await.unwrap_or_default();
    (status, body)
}

pub(crate) async fn query_snapshot_entities(client: &Client, snapshot_id: &str, query: &str) -> (StatusCode, HeaderMap, Value) {
    let response = client
        .get(broker_url(&format!("/entities?{query}")))
        .header("NGSILD-Snapshot", snapshot_id)
        .send()
        .await
        .expect("Snapshot-scoped Entity query");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json().await.unwrap_or_default();
    (status, headers, body)
}

pub(crate) async fn get_snapshot(client: &Client, id: &str) -> (StatusCode, Value) {
    let response = client.get(format!("{BASE_URL}/snapshots/{id}")).send().await.expect("get Snapshot request");
    let status = response.status();
    let body = response.json().await.unwrap_or_default();
    (status, body)
}

pub(crate) async fn update_snapshot(client: &Client, id: &str, body: &Value) -> (StatusCode, Value) {
    let response = client
        .patch(format!("{BASE_URL}/snapshots/{id}"))
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .expect("patch Snapshot request");
    let status = response.status();
    let body = response.json().await.unwrap_or_default();
    (status, body)
}

pub(crate) async fn clone_snapshot_raw(client: &Client, source_id: &str, body: &Value) -> (StatusCode, HeaderMap, Value) {
    let response = client
        .post(broker_url(&format!("/snapshots/{source_id}/clone")))
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .expect("clone Snapshot request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json().await.unwrap_or_default();
    (status, headers, body)
}

pub(crate) async fn delete_snapshot_raw(client: &Client, id: &str) -> (StatusCode, HeaderMap, Value) {
    let response = client
        .delete(broker_url(&format!("/snapshots/{id}")))
        .send()
        .await
        .expect("delete Snapshot request");
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.json().await.unwrap_or_default();
    (status, headers, body)
}

pub(crate) async fn wait_for_snapshot_terminal(client: &Client, id: &str, deadline: Duration) -> Value {
    tokio::time::timeout(deadline, async {
        let mut changes = tokio::time::interval(Duration::from_millis(20));
        changes.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            changes.tick().await;
            let (status, snapshot) = get_snapshot(client, id).await;
            pretty_assertions::assert_eq!(status, StatusCode::OK);
            if snapshot["snapshotStatus"] != "preparing" {
                return snapshot;
            }
        }
    })
    .await
    .expect("Snapshot reaches a terminal state before the test deadline")
}

pub(crate) async fn assert_snapshot_create_rejected(client: &Client, body: Value) {
    let (status, _, problem) = create_snapshot_raw(client, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
}
