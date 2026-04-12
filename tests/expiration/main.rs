//! Attribute-level `expiresAt` integration tests (ETSI GS CIM 009 clause 4.22).
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly -- no dependency
//! on the libtest-mimic harness or its fixtures module.
//!
//! Covers:
//! - `expiresAt` preservation on create/retrieve for all 8 NGSI-LD attribute types.
//! - Independence of attribute-level vs entity-level `expiresAt`.
//! - Survival of `expiresAt` across unrelated merge-patches.
//! - Visibility of `expiresAt` without the `sysAttrs` option.
//! - Background worker: partial multi-instance expiry, full attribute expiry,
//!   geo-column nulling, and entity-level/attribute-level independent expiry.
//! - `expiresAt` can be updated via merge-patch.
//! - Attributes without `expiresAt` are never removed by the worker.
//!
//! Naming convention: `expiration_<scenario>_<expected_outcome>`.

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
    let pool = PgPool::connect(DATABASE_URL).await.expect("connect test database");
    sqlx::query(TRUNCATE_ALL).execute(&pool).await.expect("truncate test tables");
    pool.close().await;
}

/// POST /entities. Returns (status, location).
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

/// GET /entities/{id} with optional query string.
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

/// PATCH /entities/{id} (merge-patch). Returns status.
async fn merge_entity(client: &Client, id: &str, body: &Value) -> StatusCode {
    client
        .patch(format!("{BASE_URL}/entities/{id}"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("merge_entity request failed")
        .status()
}

/// GET /entities with query string.
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
// expiresAt preservation on create/retrieve -- all 8 attribute types
// (NGSI-LD clause 4.22)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn expiration_property_expires_at_preserved_on_create_retrieve() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-prop");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 80,
            "expiresAt": "2099-12-31T23:59:59Z"
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "options=sysAttrs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["speed"]["expiresAt"], "2099-12-31T23:59:59Z");
}

#[tokio::test]
async fn expiration_relationship_expires_at_preserved_on_create_retrieve() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-rel");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "isParkedIn": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:ParkingSpot:1",
            "expiresAt": "2099-06-15T12:00:00Z"
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "options=sysAttrs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["isParkedIn"]["expiresAt"], "2099-06-15T12:00:00Z");
}

#[tokio::test]
async fn expiration_geo_property_expires_at_preserved_on_create_retrieve() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-geo");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.0, 52.0]},
            "expiresAt": "2099-01-01T00:00:00Z"
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "options=sysAttrs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["location"]["expiresAt"], "2099-01-01T00:00:00Z");
}

#[tokio::test]
async fn expiration_vocab_property_expires_at_preserved_on_create_retrieve() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-vocab");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "category": {
            "type": "VocabProperty",
            "vocab": "bus",
            "expiresAt": "2099-03-15T08:00:00Z"
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "options=sysAttrs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["category"]["expiresAt"], "2099-03-15T08:00:00Z");
}

#[tokio::test]
async fn expiration_language_property_expires_at_preserved_on_create_retrieve() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-lang");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "name": {
            "type": "LanguageProperty",
            "languageMap": {"en": "bus", "de": "Bus"},
            "expiresAt": "2099-07-01T00:00:00Z"
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "options=sysAttrs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"]["expiresAt"], "2099-07-01T00:00:00Z");
}

#[tokio::test]
async fn expiration_json_property_expires_at_preserved_on_create_retrieve() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-json");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "diagnostics": {
            "type": "JsonProperty",
            "json": {"engine": "ok", "brakes": "warning"},
            "expiresAt": "2099-09-01T12:00:00Z"
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "options=sysAttrs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["diagnostics"]["expiresAt"], "2099-09-01T12:00:00Z");
}

#[tokio::test]
async fn expiration_list_property_expires_at_preserved_on_create_retrieve() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-listp");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "tags": {
            "type": "ListProperty",
            "valueList": ["a", "b"],
            "expiresAt": "2099-11-20T06:30:00Z"
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "options=sysAttrs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["tags"]["expiresAt"], "2099-11-20T06:30:00Z");
}

#[tokio::test]
async fn expiration_list_relationship_expires_at_preserved_on_create_retrieve() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-listr");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "connectedTo": {
            "type": "ListRelationship",
            "objectList": ["urn:ngsi-ld:Vehicle:2", "urn:ngsi-ld:Vehicle:3"],
            "expiresAt": "2099-04-10T18:00:00Z"
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "options=sysAttrs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["connectedTo"]["expiresAt"], "2099-04-10T18:00:00Z");
}

// ---------------------------------------------------------------------------
// Independence and edge cases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn expiration_attribute_independent_of_entity_level_expires_at() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-indep");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "expiresAt": "2099-01-01T00:00:00Z",
        "speed": {
            "type": "Property",
            "value": 60,
            "expiresAt": "2098-06-01T00:00:00Z"
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "options=sysAttrs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["expiresAt"], "2099-01-01T00:00:00Z", "entity-level expiresAt");
    assert_eq!(
        body["speed"]["expiresAt"], "2098-06-01T00:00:00Z",
        "attribute-level expiresAt differs from entity-level"
    );
}

#[tokio::test]
async fn expiration_multi_instance_different_expires_at_each_preserved() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-multi");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            {
                "type": "Property",
                "value": 80,
                "datasetId": "urn:ngsi-ld:dataset:speedometer",
                "expiresAt": "2099-12-31T23:59:59Z"
            },
            {
                "type": "Property",
                "value": 78,
                "datasetId": "urn:ngsi-ld:dataset:gps",
                "expiresAt": "2098-06-15T00:00:00Z"
            }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "options=sysAttrs").await;
    assert_eq!(status, StatusCode::OK);
    let speed = body["speed"].as_array().expect("speed must be multi-instance array");
    assert_eq!(speed.len(), 2);
    let expires_values: Vec<&str> = speed.iter().filter_map(|s| s["expiresAt"].as_str()).collect();
    assert!(expires_values.contains(&"2099-12-31T23:59:59Z"), "first instance expiresAt must be preserved");
    assert!(expires_values.contains(&"2098-06-15T00:00:00Z"), "second instance expiresAt must be preserved");
}

#[tokio::test]
async fn expiration_attribute_survives_unrelated_merge_patch() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-surv");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 50,
            "expiresAt": "2099-12-31T23:59:59Z"
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Merge-patch adds an unrelated attribute; speed expiresAt must survive.
    let patch = json!({
        "fuel": {
            "type": "Property",
            "value": 75
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = merge_entity(&c, &id, &patch).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(&c, &id, "options=sysAttrs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["speed"]["expiresAt"], "2099-12-31T23:59:59Z",
        "expiresAt must survive merge-patch of unrelated attrs"
    );
}

#[tokio::test]
async fn expiration_expires_at_visible_without_sys_attrs_option() {
    // expiresAt is a user-settable member, not a system-only field; it must
    // appear in the response even without ?options=sysAttrs (clause 4.22).
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-nosys");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 90,
            "expiresAt": "2099-12-31T23:59:59Z"
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["speed"]["expiresAt"], "2099-12-31T23:59:59Z", "expiresAt must be visible without sysAttrs");
}

#[tokio::test]
async fn expiration_expires_at_updatable_via_merge_patch() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-update");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 60,
            "expiresAt": "2099-01-01T00:00:00Z"
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let patch = json!({
        "speed": {
            "type": "Property",
            "value": 60,
            "expiresAt": "2099-06-30T00:00:00Z"
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = merge_entity(&c, &id, &patch).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(&c, &id, "options=sysAttrs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["speed"]["expiresAt"], "2099-06-30T00:00:00Z", "expiresAt must reflect merge-patch value");
}

// ---------------------------------------------------------------------------
// Worker-driven expiration tests
// Each test waits for one worker cycle (~12 s). Serial to prevent interference.
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn expiration_worker_partial_multi_instance_expired_instance_removed() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-part");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            {
                "type": "Property",
                "value": 80,
                "datasetId": "urn:ngsi-ld:dataset:a",
                "expiresAt": "2020-01-01T00:00:00Z"
            },
            {
                "type": "Property",
                "value": 78,
                "datasetId": "urn:ngsi-ld:dataset:b",
                "expiresAt": "2099-12-31T23:59:59Z"
            },
            {
                "type": "Property",
                "value": 75,
                "datasetId": "urn:ngsi-ld:dataset:c"
            }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    tokio::time::sleep(std::time::Duration::from_secs(12)).await;

    let (status, body) = get_entity(&c, &id, "options=sysAttrs").await;
    assert_eq!(status, StatusCode::OK);
    let speed = body["speed"].as_array().expect("speed must be multi-instance array");
    assert_eq!(speed.len(), 2, "expired instance must be removed, 2 instances remain");
}

#[tokio::test]
#[serial]
async fn expiration_worker_all_instances_expired_attribute_absent_from_entity() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-allone");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 80,
            "expiresAt": "2020-01-01T00:00:00Z"
        },
        "fuel": {
            "type": "Property",
            "value": 55
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    tokio::time::sleep(std::time::Duration::from_secs(12)).await;

    let (status, body) = get_entity(&c, &id, "options=sysAttrs").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_none(), "fully-expired attribute must be absent");
    assert!(body.get("fuel").is_some(), "attribute without expiresAt must remain");
}

#[tokio::test]
#[serial]
async fn expiration_worker_attribute_without_expires_at_never_removed() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-noexp");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 60
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    tokio::time::sleep(std::time::Duration::from_secs(12)).await;

    let (status, body) = get_entity(&c, &id, "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_some(), "attribute without expiresAt must never be removed by worker");
}

#[tokio::test]
#[serial]
async fn expiration_worker_expired_location_removed_and_excluded_from_geo_queries() {
    // When the location GeoProperty expires, its geo-column must be nulled so
    // geo-proximity queries no longer match the entity.
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-geo-null");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.0, 52.0]},
            "expiresAt": "2020-01-01T00:00:00Z"
        },
        "speed": {
            "type": "Property",
            "value": 60
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    tokio::time::sleep(std::time::Duration::from_secs(12)).await;

    let (status, body) = get_entity(&c, &id, "options=sysAttrs").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("location").is_none(), "expired location must be removed from entity");
    assert!(body.get("speed").is_some(), "non-expired attribute must remain");

    // Geo query centred on the former coordinates must not return this entity.
    let (status, body) = query_entities(
        &c,
        "type=Vehicle&georel=near;maxDistance%3D%3D1000&geometry=Point&coordinates=%5B13.0%2C52.0%5D",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let entities = body.as_array().expect("geo query must return JSON array");
    let matched = entities.iter().any(|e| e["id"].as_str() == Some(id.as_str()));
    assert!(!matched, "entity with expired location must not appear in geo query results");
}

#[tokio::test]
#[serial]
async fn expiration_worker_attribute_expiry_does_not_affect_entity_level_expires_at() {
    // Entity-level expiresAt is far future; only the attribute must expire.
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:exp-entattr");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "expiresAt": "2099-12-31T23:59:59Z",
        "speed": {
            "type": "Property",
            "value": 80,
            "expiresAt": "2020-01-01T00:00:00Z"
        },
        "fuel": {
            "type": "Property",
            "value": 50
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    tokio::time::sleep(std::time::Duration::from_secs(12)).await;

    let (status, body) = get_entity(&c, &id, "options=sysAttrs").await;
    assert_eq!(status, StatusCode::OK, "entity must still exist after attribute expiry");
    assert!(body.get("speed").is_none(), "attribute with past expiresAt must be removed");
    assert!(body.get("fuel").is_some(), "attribute without expiresAt must remain");
    assert_eq!(body["expiresAt"], "2099-12-31T23:59:59Z", "entity-level expiresAt must be unaffected");
}
