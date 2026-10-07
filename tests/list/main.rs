//! `ListProperty` and `ListRelationship` integration tests (NGSI-LD 5.2.36, 5.2.37).
//!
//! Naming convention: `list_<type>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

async fn create(client: &TestClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED);
}

async fn fetch(client: &TestClient, id: &str, query: &str) -> Value {
    let response = client
        .get_entity(id, query, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("get_entity failed");
    assert_eq!(response.status, StatusCode::OK);
    response.json_value().expect("entity response must be valid JSON")
}

async fn query(client: &TestClient, query_str: &str) -> Value {
    let response = client
        .query_entities(query_str, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities failed");
    assert_eq!(response.status, StatusCode::OK);
    response.json_value().expect("entity query response must be valid JSON")
}

#[tokio::test]
async fn list_property_create_retrieve_normalized_returns_list_property_with_value_list() {
    let scope = TestEnvironment::from_suite("list").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:lp1");
    let entity = json!({
        "id": id,
        "type": "Building",
        "tags": {"type": "ListProperty", "valueList": ["residential", "commercial", "mixed-use"]},
        "@context": [TEST_SUITE_CONTEXT]
    });
    create(client, &entity).await;

    let body = fetch(client, &id, "").await;
    let tags = &body["tags"];
    assert_eq!(tags["type"], "ListProperty", "attribute type must be ListProperty");
    let value_list = tags["valueList"].as_array().expect("valueList must be a JSON array");
    assert_eq!(value_list.len(), 3);
    assert_eq!(value_list[0], "residential");
    assert_eq!(value_list[2], "mixed-use");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_property_retrieve_key_values_returns_array() {
    let scope = TestEnvironment::from_suite("list").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:lp2");
    let entity = json!({
        "id": id,
        "type": "Building",
        "tags": {"type": "ListProperty", "valueList": [1, 2, 3]},
        "@context": [TEST_SUITE_CONTEXT]
    });
    create(client, &entity).await;

    let body = fetch(client, &id, "options=keyValues").await;
    let tags = body["tags"].as_array().expect("simplified ListProperty must be a JSON array");
    assert_eq!(tags.len(), 3);
    assert_eq!(tags[0], 1);
    assert_eq!(tags[2], 3);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_property_string_values_round_trip_through_create_and_retrieve() {
    let scope = TestEnvironment::from_suite("list").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:lp3");
    let entity = json!({
        "id": id,
        "type": "Building",
        "labels": {"type": "ListProperty", "valueList": ["alpha", "beta", "gamma", "delta"]},
        "@context": [TEST_SUITE_CONTEXT]
    });
    create(client, &entity).await;

    let body = fetch(client, &id, "").await;
    let value_list = body["labels"]["valueList"].as_array().expect("valueList missing");
    assert_eq!(value_list.len(), 4);
    assert_eq!(value_list[0], "alpha");
    assert_eq!(value_list[3], "delta");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_relationship_create_retrieve_normalized_returns_list_relationship_with_object_list() {
    let scope = TestEnvironment::from_suite("list").await.expect("isolated test environment");
    let client = scope.client();

    let target_a = scope.id("Room:A");
    let target_b = scope.id("Room:B");
    for (tid, name) in [(&target_a, "Room A"), (&target_b, "Room B")] {
        create(
            client,
            &json!({
                "id": tid,
                "type": "Room",
                "name": {"type": "Property", "value": name},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
    }

    let id = scope.id("Building:lr1");
    create(
        client,
        &json!({
            "id": id,
            "type": "Building",
            "hasRooms": {"type": "ListRelationship", "objectList": [target_a, target_b]},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let body = fetch(client, &id, "").await;
    assert_eq!(body["hasRooms"]["type"], "ListRelationship");
    let obj_list = body["hasRooms"]["objectList"].as_array().expect("objectList must be an array");
    assert_eq!(obj_list.len(), 2);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_relationship_retrieve_key_values_returns_array_of_object_ids() {
    let scope = TestEnvironment::from_suite("list").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:lrs");
    create(
        client,
        &json!({
            "id": id,
            "type": "Building",
            "hasRooms": {
                "type": "ListRelationship",
                "objectList": ["urn:ngsi-ld:Room:X", "urn:ngsi-ld:Room:Y"]
            },
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let body = fetch(client, &id, "options=keyValues").await;
    let has_rooms = body["hasRooms"].as_array().expect("simplified ListRelationship must be an array");
    assert_eq!(has_rooms.len(), 2);
    assert_eq!(has_rooms[0], "urn:ngsi-ld:Room:X");
    assert_eq!(has_rooms[1], "urn:ngsi-ld:Room:Y");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_relationship_join_flat_includes_linked_entities_alongside_source() {
    let scope = TestEnvironment::from_suite("list").await.expect("isolated test environment");
    let client = scope.client();

    let target_a = scope.id("Sensor:A");
    let target_b = scope.id("Sensor:B");
    let target_c = scope.id("Sensor:C");
    for (tid, name) in [(&target_a, "A"), (&target_b, "B"), (&target_c, "C")] {
        create(
            client,
            &json!({
                "id": tid,
                "type": "Sensor",
                "name": {"type": "Property", "value": name},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
    }

    let id = scope.id("Building:lrjf");
    create(
        client,
        &json!({
            "id": id,
            "type": "Building",
            "hasSensors": {"type": "ListRelationship", "objectList": [target_a, target_b, target_c]},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let body = query(client, &format!("id={id}&type=Building&join=flat&joinLevel=1")).await;
    let arr = body.as_array().expect("join=flat must return an array");
    assert!(arr.len() >= 4, "result must contain source + 3 sensors, got {}", arr.len());
    let ids: Vec<&str> = arr.iter().filter_map(|item| item["id"].as_str()).collect();
    assert!(ids.contains(&target_a.as_str()));
    assert!(ids.contains(&target_b.as_str()));
    assert!(ids.contains(&target_c.as_str()));
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_relationship_join_inline_embeds_linked_entities_in_attribute() {
    let scope = TestEnvironment::from_suite("list").await.expect("isolated test environment");
    let client = scope.client();

    let target_a = scope.id("Device:inlA");
    let target_b = scope.id("Device:inlB");
    for (tid, name) in [(&target_a, "DevA"), (&target_b, "DevB")] {
        create(
            client,
            &json!({
                "id": tid,
                "type": "Device",
                "name": {"type": "Property", "value": name},
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await;
    }

    let id = scope.id("Building:lrinl");
    create(
        client,
        &json!({
            "id": id,
            "type": "Building",
            "hasDevices": {"type": "ListRelationship", "objectList": [target_a, target_b]},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let body = fetch(client, &id, "join=inline&joinLevel=1").await;
    assert_eq!(body["hasDevices"]["type"], "ListRelationship");
    let entity_list = body["hasDevices"]["entityList"].as_array().expect("entityList must be an array");
    assert_eq!(entity_list.len(), 2);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn list_relationship_with_nonexistent_targets_still_creates_with_full_object_list() {
    let scope = TestEnvironment::from_suite("list").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:lrnx");
    create(
        client,
        &json!({
            "id": id,
            "type": "Building",
            "hasRooms": {
                "type": "ListRelationship",
                "objectList": ["urn:ngsi-ld:Room:NonExistent1", "urn:ngsi-ld:Room:NonExistent2"]
            },
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let body = fetch(client, &id, "").await;
    let obj_list = body["hasRooms"]["objectList"].as_array().expect("objectList missing");
    assert_eq!(obj_list.len(), 2);
    scope.finish().await.expect("cleanup failed");
}
