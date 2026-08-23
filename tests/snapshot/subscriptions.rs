//! Snapshot-scoped subscription and notification acceptance cases.

use super::*;
use crate::notification_adapter::NotificationAdapter;
use reqwest::{Method, Response};

const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";
const ENTITY_TYPE: &str = "https://example.org/SubscriptionVehicle";

fn entity(id: &str, value: i64) -> Value {
    json!({"id": id, "type": ENTITY_TYPE, "speed": {"type": "Property", "value": value}, "@context": CORE_CONTEXT})
}

fn subscription(id: &str, endpoint: &str) -> Value {
    json!({"id": id, "type": "Subscription", "entities": [{"type": ENTITY_TYPE}], "notification": {"format": "normalized", "endpoint": {"uri": endpoint, "accept": "application/json"}}, "@context": CORE_CONTEXT})
}

async fn empty_branch(id: &str) {
    pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &json!({"id": id, "type": "Snapshot", "snapshotQueries": []})).await.0, StatusCode::CREATED);
    wait_for_snapshot_terminal(&client(), id, Duration::from_secs(5)).await;
}

async fn scoped(method: Method, path: &str, snapshot: &str, body: Option<&Value>) -> Response {
    let mut request = client().request(method, broker_url(path)).header("NGSILD-Snapshot", snapshot);
    if let Some(body) = body {
        request = request.header("Content-Type", "application/ld+json").json(body);
    }
    request.send().await.expect("Snapshot-scoped subscription request")
}

async fn create_scoped_subscription(snapshot: &str, body: &Value) -> Response {
    let response = scoped(Method::POST, "/subscriptions", snapshot, Some(body)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    response
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_subscription_crud_lifecycle_is_partitioned() {
    let _serialized_environment = clean_database().await;
    let first = "urn:ngsi-ld:Snapshot:subscription-crud-first";
    let second = "urn:ngsi-ld:Snapshot:subscription-crud-second";
    let subscription_id = "urn:ngsi-ld:Subscription:snapshot-crud";
    empty_branch(first).await;
    empty_branch(second).await;
    let created = create_scoped_subscription(first, &subscription(subscription_id, "http://host.docker.internal:9/notify")).await;
    pretty_assertions::assert_eq!(created.status(), StatusCode::CREATED);
    pretty_assertions::assert_eq!(created.headers()["NGSILD-Snapshot"], first);
    pretty_assertions::assert_eq!(scoped(Method::GET, &format!("/subscriptions/{subscription_id}"), first, None).await.status(), StatusCode::OK);
    pretty_assertions::assert_eq!(scoped(Method::GET, &format!("/subscriptions/{subscription_id}"), second, None).await.status(), StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(client().get(broker_url(&format!("/subscriptions/{subscription_id}"))).send().await.expect("live subscription get").status(), StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(scoped(Method::GET, "/subscriptions", first, None).await.status(), StatusCode::OK);
    pretty_assertions::assert_eq!(scoped(Method::PATCH, &format!("/subscriptions/{subscription_id}"), first, Some(&json!({"isActive": false, "@context": CORE_CONTEXT}))).await.status(), StatusCode::NO_CONTENT);
    pretty_assertions::assert_eq!(scoped(Method::DELETE, &format!("/subscriptions/{subscription_id}"), first, None).await.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_subscription_branch_mutation_delivers_data_and_snapshot_header() {
    let _serialized_environment = clean_database().await;
    let receiver = NotificationAdapter::start().await;
    let snapshot = "urn:ngsi-ld:Snapshot:subscription-delivery";
    let subscription_id = "urn:ngsi-ld:Subscription:snapshot-delivery";
    empty_branch(snapshot).await;
    pretty_assertions::assert_eq!(create_scoped_subscription(snapshot, &subscription(subscription_id, &receiver.endpoint_url())).await.status(), StatusCode::CREATED);
    receiver.clear_notifications().await;
    pretty_assertions::assert_eq!(scoped(Method::POST, "/entities", snapshot, Some(&entity("urn:ngsi-ld:SubscriptionVehicle:delivery", 1))).await.status(), StatusCode::CREATED);
    let notification = receiver.wait_for_subscription_notification(subscription_id, Duration::from_secs(10)).await;
    pretty_assertions::assert_eq!(notification.headers["NGSILD-Snapshot"], snapshot);
    assert!(notification.body["data"].to_string().contains("SubscriptionVehicle:delivery"));
    receiver.shutdown().await;
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_subscription_ignores_live_and_sibling_branch_mutations() {
    let _serialized_environment = clean_database().await;
    let receiver = NotificationAdapter::start().await;
    let first = "urn:ngsi-ld:Snapshot:subscription-isolation-first";
    let second = "urn:ngsi-ld:Snapshot:subscription-isolation-second";
    let subscription_id = "urn:ngsi-ld:Subscription:snapshot-isolation";
    empty_branch(first).await;
    empty_branch(second).await;
    pretty_assertions::assert_eq!(create_scoped_subscription(first, &subscription(subscription_id, &receiver.endpoint_url())).await.status(), StatusCode::CREATED);
    pretty_assertions::assert_eq!(create_entity(&client(), &entity("urn:ngsi-ld:SubscriptionVehicle:live", 1)).await, StatusCode::CREATED);
    pretty_assertions::assert_eq!(scoped(Method::POST, "/entities", second, Some(&entity("urn:ngsi-ld:SubscriptionVehicle:sibling", 2))).await.status(), StatusCode::CREATED);
    receiver.wait_for_no_notification_for_subscription(subscription_id, Duration::from_millis(750)).await;
    receiver.shutdown().await;
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_subscription_update_and_delete_change_future_delivery() {
    let _serialized_environment = clean_database().await;
    let receiver = NotificationAdapter::start().await;
    let snapshot = "urn:ngsi-ld:Snapshot:subscription-update";
    let subscription_id = "urn:ngsi-ld:Subscription:snapshot-update";
    empty_branch(snapshot).await;
    pretty_assertions::assert_eq!(create_scoped_subscription(snapshot, &subscription(subscription_id, &receiver.endpoint_url())).await.status(), StatusCode::CREATED);
    pretty_assertions::assert_eq!(scoped(Method::PATCH, &format!("/subscriptions/{subscription_id}"), snapshot, Some(&json!({"isActive": false, "@context": CORE_CONTEXT}))).await.status(), StatusCode::NO_CONTENT);
    pretty_assertions::assert_eq!(scoped(Method::POST, "/entities", snapshot, Some(&entity("urn:ngsi-ld:SubscriptionVehicle:inactive", 1))).await.status(), StatusCode::CREATED);
    receiver.wait_for_no_subscription_notification(subscription_id, Duration::from_millis(500)).await;
    pretty_assertions::assert_eq!(scoped(Method::PATCH, &format!("/subscriptions/{subscription_id}"), snapshot, Some(&json!({"isActive": true, "@context": CORE_CONTEXT}))).await.status(), StatusCode::NO_CONTENT);
    tokio::time::sleep(Duration::from_millis(200)).await;
    pretty_assertions::assert_eq!(scoped(Method::POST, "/entities", snapshot, Some(&entity("urn:ngsi-ld:SubscriptionVehicle:active", 2))).await.status(), StatusCode::CREATED);
    receiver.wait_for_subscription_notification(subscription_id, Duration::from_secs(10)).await;
    pretty_assertions::assert_eq!(scoped(Method::DELETE, &format!("/subscriptions/{subscription_id}"), snapshot, None).await.status(), StatusCode::NO_CONTENT);
    pretty_assertions::assert_eq!(scoped(Method::POST, "/entities", snapshot, Some(&entity("urn:ngsi-ld:SubscriptionVehicle:deleted", 3))).await.status(), StatusCode::CREATED);
    receiver.wait_for_no_subscription_notification(subscription_id, Duration::from_millis(500)).await;
    receiver.shutdown().await;
}
