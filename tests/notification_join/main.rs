//! Subscription notification joinLevel integration tests (NGSI-LD 5.2.14.1).
//!
//! Naming convention: `notification_join_level_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

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
    tokio::time::sleep(Duration::from_millis(150)).await;
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

fn trigger_fragment() -> Value {
    json!({"name": {"type": "Property", "value": "Updated"}, "@context": [CORE_CONTEXT]})
}

#[tokio::test]
async fn notification_join_level_3_delivers_three_depth_levels_of_linked_entities() {
    let scope = TestEnvironment::from_suite("notification_join").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");

    let country_id = scope.id("Country:jl3-France");
    create(
        client,
        &json!({
            "id": country_id,
            "type": "Country",
            "name": {"type": "Property", "value": "France"},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let city_id = scope.id("City:jl3-Paris");
    create(
        client,
        &json!({
            "id": city_id,
            "type": "City",
            "name": {"type": "Property", "value": "Paris"},
            "locatedIn": {"type": "Relationship", "object": country_id},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let building_id = scope.id("Building:jl3");
    create(
        client,
        &json!({
            "id": building_id,
            "type": "Building",
            "locatedAt": {"type": "Relationship", "object": city_id},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let subscription_id = scope.id("Subscription:jl3");
    let sub = subscription_with_flat_join(
        &subscription_id,
        &building_id,
        &receiver.endpoint_url().expect("notification endpoint must be valid"),
        Some(3),
    );
    create_subscription(client, &sub).await;

    append_attrs(client, &building_id, &trigger_fragment()).await;

    let notification = receiver.next_request(Duration::from_secs(5)).await.expect("notification must arrive");
    let notification_body: Value = notification.json().expect("notification must be valid JSON");
    assert_eq!(notification_body["subscriptionId"], subscription_id);
    let data = notification_body["data"].as_array().expect("data must be an array");
    assert!(data.len() >= 3, "joinLevel=3 must include building + city + country, got {}", data.len());
    let ids: Vec<&str> = data.iter().filter_map(|d| d["id"].as_str()).collect();
    assert!(ids.contains(&building_id.as_str()));
    assert!(ids.contains(&city_id.as_str()));
    assert!(ids.contains(&country_id.as_str()));
    receiver.shutdown().await.expect("notification server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_join_level_default_is_1_when_omitted_from_subscription() {
    let scope = TestEnvironment::from_suite("notification_join").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");

    let country_id = scope.id("Country:jldef-Germany");
    create(
        client,
        &json!({
            "id": country_id,
            "type": "Country",
            "name": {"type": "Property", "value": "Germany"},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let city_id = scope.id("City:jldef-Berlin");
    create(
        client,
        &json!({
            "id": city_id,
            "type": "City",
            "locatedIn": {"type": "Relationship", "object": country_id},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let building_id = scope.id("Building:jldef");
    create(
        client,
        &json!({
            "id": building_id,
            "type": "Building",
            "locatedAt": {"type": "Relationship", "object": city_id},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let subscription_id = scope.id("Subscription:jldef");
    let sub = subscription_with_flat_join(
        &subscription_id,
        &building_id,
        &receiver.endpoint_url().expect("notification endpoint must be valid"),
        None,
    );
    create_subscription(client, &sub).await;

    append_attrs(client, &building_id, &trigger_fragment()).await;

    let notification = receiver.next_request(Duration::from_secs(5)).await.expect("notification must arrive");
    let notification_body: Value = notification.json().expect("notification must be valid JSON");
    assert_eq!(notification_body["subscriptionId"], subscription_id);
    let data = notification_body["data"].as_array().expect("data must be an array");
    assert_eq!(data.len(), 2, "default joinLevel=1 must return building + city");
    let ids: Vec<&str> = data.iter().filter_map(|d| d["id"].as_str()).collect();
    assert!(ids.contains(&building_id.as_str()));
    assert!(ids.contains(&city_id.as_str()));
    assert!(!ids.contains(&country_id.as_str()), "country at depth 2 must not appear with default joinLevel");
    receiver.shutdown().await.expect("notification server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_join_level_1_delivers_one_depth_level_of_linked_entities() {
    let scope = TestEnvironment::from_suite("notification_join").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");

    let country_id = scope.id("Country:jl1-Spain");
    create(
        client,
        &json!({
            "id": country_id,
            "type": "Country",
            "name": {"type": "Property", "value": "Spain"},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let city_id = scope.id("City:jl1-Madrid");
    create(
        client,
        &json!({
            "id": city_id,
            "type": "City",
            "locatedIn": {"type": "Relationship", "object": country_id},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let building_id = scope.id("Building:jl1");
    create(
        client,
        &json!({
            "id": building_id,
            "type": "Building",
            "locatedAt": {"type": "Relationship", "object": city_id},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let subscription_id = scope.id("Subscription:jl1");
    let sub = subscription_with_flat_join(
        &subscription_id,
        &building_id,
        &receiver.endpoint_url().expect("notification endpoint must be valid"),
        Some(1),
    );
    create_subscription(client, &sub).await;

    append_attrs(client, &building_id, &trigger_fragment()).await;

    let notification = receiver.next_request(Duration::from_secs(5)).await.expect("notification must arrive");
    let notification_body: Value = notification.json().expect("notification must be valid JSON");
    assert_eq!(notification_body["subscriptionId"], subscription_id);
    let data = notification_body["data"].as_array().expect("data must be an array");
    assert_eq!(data.len(), 2, "joinLevel=1 must return building + directly-linked city");
    let ids: Vec<&str> = data.iter().filter_map(|d| d["id"].as_str()).collect();
    assert!(ids.contains(&building_id.as_str()));
    assert!(ids.contains(&city_id.as_str()));
    assert!(!ids.contains(&country_id.as_str()));
    receiver.shutdown().await.expect("notification server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn notification_join_level_2_delivers_two_depth_levels_of_linked_entities() {
    let scope = TestEnvironment::from_suite("notification_join").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");

    let continent_id = scope.id("Continent:jl2-Europe");
    create(
        client,
        &json!({
            "id": continent_id,
            "type": "Continent",
            "name": {"type": "Property", "value": "Europe"},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let country_id = scope.id("Country:jl2-Italy");
    create(
        client,
        &json!({
            "id": country_id,
            "type": "Country",
            "name": {"type": "Property", "value": "Italy"},
            "partOf": {"type": "Relationship", "object": continent_id},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let city_id = scope.id("City:jl2-Rome");
    create(
        client,
        &json!({
            "id": city_id,
            "type": "City",
            "locatedIn": {"type": "Relationship", "object": country_id},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let building_id = scope.id("Building:jl2");
    create(
        client,
        &json!({
            "id": building_id,
            "type": "Building",
            "locatedAt": {"type": "Relationship", "object": city_id},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let subscription_id = scope.id("Subscription:jl2");
    let sub = subscription_with_flat_join(
        &subscription_id,
        &building_id,
        &receiver.endpoint_url().expect("notification endpoint must be valid"),
        Some(2),
    );
    create_subscription(client, &sub).await;

    append_attrs(client, &building_id, &trigger_fragment()).await;

    let notification = receiver.next_request(Duration::from_secs(5)).await.expect("notification must arrive");
    let notification_body: Value = notification.json().expect("notification must be valid JSON");
    assert_eq!(notification_body["subscriptionId"], subscription_id);
    let data = notification_body["data"].as_array().expect("data must be an array");
    assert_eq!(data.len(), 3, "joinLevel=2 must return building + city + country");
    let ids: Vec<&str> = data.iter().filter_map(|d| d["id"].as_str()).collect();
    assert!(ids.contains(&building_id.as_str()));
    assert!(ids.contains(&city_id.as_str()));
    assert!(ids.contains(&country_id.as_str()));
    assert!(!ids.contains(&continent_id.as_str()));
    receiver.shutdown().await.expect("notification server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}
