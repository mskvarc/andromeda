//! ListProperty and ListRelationship integration tests for the NGSI-LD context broker.
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly -- no dependency
//! on the libtest-mimic harness or its fixtures module.
//!
//! Covers spec sections 5.2.36 (ListProperty) and 5.2.37 (ListRelationship):
//! - Normalized create/retrieve round-trips
//! - Simplified (keyValues) representation
//! - Join operations (flat and inline) on ListRelationship
//!
//! Naming convention: `list_<type>_<scenario>_<expected_outcome>`.

// Test infrastructure legitimately uses expect/unwrap to fail fast on transport errors.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
const TEST_SUITE_CONTEXT: &str = "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite.jsonld";

const TRUNCATE_ALL: &str = "\
    TRUNCATE \
        entities, \
        jsonld_contexts, \
        temporal_entity_attributes, \
        attribute_history, \
        subscriptions, \
        context_source_registrations, \
        snapshots \
    CASCADE";

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

fn link_header() -> String {
    format!("<{TEST_SUITE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

async fn clean_database() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("failed to connect to database for cleanup");
    sqlx::query(TRUNCATE_ALL).execute(&pool).await.expect("failed to truncate tables");
    pool.close().await;
}

/// POST /entities with application/ld+json body. Returns (status, location).
async fn create_entity(client: &Client, body: &Value) -> (StatusCode, Option<String>) {
    let resp = client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create_entity request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

/// DELETE /entities/{id}.
async fn delete_entity(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/entities/{id}")).send().await;
}

/// GET /entities/{id} with Accept: application/ld+json, optional extra query params.
/// Returns (status, body).
async fn get_entity(client: &Client, id: &str, query: &str) -> (StatusCode, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities/{id}{sep}{query}"))
        .header("Accept", "application/ld+json")
        .header("Link", link_header())
        .send()
        .await
        .expect("get_entity request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// GET /entities with Accept: application/ld+json and caller-supplied query string.
/// Returns (status, body).
async fn query_entities(client: &Client, query: &str) -> (StatusCode, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities{sep}{query}"))
        .header("Accept", "application/ld+json")
        .header("Link", link_header())
        .send()
        .await
        .expect("query_entities request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

// ---------------------------------------------------------------------------
// ListProperty (spec 5.2.36)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn list_property_create_retrieve_normalized_returns_list_property_with_value_list() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lp1");
    let entity = json!({
        "id": id,
        "type": "Building",
        "tags": {
            "type": "ListProperty",
            "valueList": ["residential", "commercial", "mixed-use"]
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let tags = &body["tags"];
    pretty_assertions::assert_eq!(tags["type"], "ListProperty", "attribute type must be ListProperty");

    let value_list = tags["valueList"].as_array().expect("valueList must be a JSON array");
    pretty_assertions::assert_eq!(value_list.len(), 3, "all three valueList items must be preserved");
    pretty_assertions::assert_eq!(value_list[0], "residential");
    pretty_assertions::assert_eq!(value_list[1], "commercial");
    pretty_assertions::assert_eq!(value_list[2], "mixed-use");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn list_property_retrieve_key_values_returns_array() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lp2");
    let entity = json!({
        "id": id,
        "type": "Building",
        "tags": {
            "type": "ListProperty",
            "valueList": [1, 2, 3]
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "options=keyValues").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let tags = body["tags"].as_array().expect("simplified ListProperty must be a JSON array");
    pretty_assertions::assert_eq!(tags.len(), 3, "all three values must be present in simplified form");
    pretty_assertions::assert_eq!(tags[0], 1);
    pretty_assertions::assert_eq!(tags[1], 2);
    pretty_assertions::assert_eq!(tags[2], 3);

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn list_property_string_values_round_trip_through_create_and_retrieve() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lp3");
    let entity = json!({
        "id": id,
        "type": "Building",
        "labels": {
            "type": "ListProperty",
            "valueList": ["alpha", "beta", "gamma", "delta"]
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let value_list = body["labels"]["valueList"].as_array().expect("valueList must be present after round-trip");
    pretty_assertions::assert_eq!(value_list.len(), 4, "all four string values must survive round-trip");
    pretty_assertions::assert_eq!(value_list[0], "alpha");
    pretty_assertions::assert_eq!(value_list[3], "delta");

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// ListRelationship (spec 5.2.37)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn list_relationship_create_retrieve_normalized_returns_list_relationship_with_object_list() {
    clean_database().await;
    let c = client();

    let target_a = unique_id("urn:ngsi-ld:Room:A");
    let target_b = unique_id("urn:ngsi-ld:Room:B");

    for (tid, name) in [(&target_a, "Room A"), (&target_b, "Room B")] {
        let e = json!({
            "id": tid, "type": "Room",
            "name": {"type": "Property", "value": name},
            "@context": [TEST_SUITE_CONTEXT]
        });
        let (status, _) = create_entity(&c, &e).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let id = unique_id("urn:ngsi-ld:Building:lr1");
    let entity = json!({
        "id": id,
        "type": "Building",
        "hasRooms": {
            "type": "ListRelationship",
            "objectList": [target_a, target_b]
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let has_rooms = &body["hasRooms"];
    pretty_assertions::assert_eq!(has_rooms["type"], "ListRelationship", "attribute type must be ListRelationship");

    let obj_list = has_rooms["objectList"].as_array().expect("objectList must be a JSON array");
    pretty_assertions::assert_eq!(obj_list.len(), 2, "both target entity IDs must be present in objectList");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn list_relationship_retrieve_key_values_returns_array_of_object_ids() {
    clean_database().await;
    let c = client();

    let id = unique_id("urn:ngsi-ld:Building:lrs");
    let entity = json!({
        "id": id,
        "type": "Building",
        "hasRooms": {
            "type": "ListRelationship",
            "objectList": ["urn:ngsi-ld:Room:X", "urn:ngsi-ld:Room:Y"]
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "options=keyValues").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let has_rooms = body["hasRooms"]
        .as_array()
        .expect("simplified ListRelationship must be a JSON array of object IDs");
    pretty_assertions::assert_eq!(has_rooms.len(), 2, "both object IDs must be present in simplified objectList");
    pretty_assertions::assert_eq!(has_rooms[0], "urn:ngsi-ld:Room:X");
    pretty_assertions::assert_eq!(has_rooms[1], "urn:ngsi-ld:Room:Y");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn list_relationship_join_flat_includes_linked_entities_alongside_source() {
    clean_database().await;
    let c = client();

    let target_a = unique_id("urn:ngsi-ld:Sensor:A");
    let target_b = unique_id("urn:ngsi-ld:Sensor:B");
    let target_c = unique_id("urn:ngsi-ld:Sensor:C");

    for (tid, name) in [(&target_a, "A"), (&target_b, "B"), (&target_c, "C")] {
        let e = json!({
            "id": tid, "type": "Sensor",
            "name": {"type": "Property", "value": name},
            "@context": [TEST_SUITE_CONTEXT]
        });
        let (status, _) = create_entity(&c, &e).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let id = unique_id("urn:ngsi-ld:Building:lrjf");
    let entity = json!({
        "id": id,
        "type": "Building",
        "hasSensors": {
            "type": "ListRelationship",
            "objectList": [target_a, target_b, target_c]
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = query_entities(&c, &format!("id={id}&type=Building&join=flat&joinLevel=1")).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("join=flat must return a JSON array");
    assert!(
        arr.len() >= 4,
        "result must contain the source building plus all 3 linked sensors, got {}",
        arr.len()
    );

    let ids: Vec<&str> = arr.iter().filter_map(|e| e["id"].as_str()).collect();
    assert!(ids.contains(&target_a.as_str()), "linked sensor A must appear in flat result");
    assert!(ids.contains(&target_b.as_str()), "linked sensor B must appear in flat result");
    assert!(ids.contains(&target_c.as_str()), "linked sensor C must appear in flat result");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn list_relationship_join_inline_embeds_linked_entities_in_attribute() {
    clean_database().await;
    let c = client();

    let target_a = unique_id("urn:ngsi-ld:Device:inlA");
    let target_b = unique_id("urn:ngsi-ld:Device:inlB");

    for (tid, name) in [(&target_a, "DevA"), (&target_b, "DevB")] {
        let e = json!({
            "id": tid, "type": "Device",
            "name": {"type": "Property", "value": name},
            "@context": [TEST_SUITE_CONTEXT]
        });
        let (status, _) = create_entity(&c, &e).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let id = unique_id("urn:ngsi-ld:Building:lrinl");
    let entity = json!({
        "id": id,
        "type": "Building",
        "hasDevices": {
            "type": "ListRelationship",
            "objectList": [target_a, target_b]
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "join=inline&joinLevel=1").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let has_devices = &body["hasDevices"];
    pretty_assertions::assert_eq!(
        has_devices["type"],
        "ListRelationship",
        "attribute type must remain ListRelationship after inline join"
    );

    let entity_list = has_devices["entityList"]
        .as_array()
        .expect("join=inline on ListRelationship must produce entityList array");
    pretty_assertions::assert_eq!(entity_list.len(), 2, "entityList must contain both linked Device entities");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn list_relationship_with_nonexistent_targets_still_creates_with_full_object_list() {
    clean_database().await;
    let c = client();

    // Targets are referenced but not created -- broker must still accept the entity
    let id = unique_id("urn:ngsi-ld:Building:lrnx");
    let entity = json!({
        "id": id,
        "type": "Building",
        "hasRooms": {
            "type": "ListRelationship",
            "objectList": [
                "urn:ngsi-ld:Room:NonExistent1",
                "urn:ngsi-ld:Room:NonExistent2"
            ]
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(
        status,
        StatusCode::CREATED,
        "ListRelationship with unresolvable targets must still be accepted (no referential integrity)"
    );

    let (status, body) = get_entity(&c, &id, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let obj_list = body["hasRooms"]["objectList"]
        .as_array()
        .expect("objectList must be present even for non-existent targets");
    pretty_assertions::assert_eq!(obj_list.len(), 2, "objectList must contain all referenced IDs even if targets do not exist");

    delete_entity(&c, &id).await;
}
