//! EntityMap integration tests for the NGSI-LD context broker.
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly.
//! No libtest-mimic and no fixture loading helpers are used.
//!
//! Naming convention: `entity_map_<area>_<scenario>_<expected_outcome>`.

// Test infrastructure legitimately uses expect/unwrap to fail fast on transport errors.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_common::{db::TRUNCATE_ALL, prelude::*};
use chrono::{Duration, Utc};
use reqwest::{Client, Method, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use urlencoding::encode;
use uuid::Uuid;

const INVALID_REQUEST_TYPE: &str = error::INVALID_REQUEST;
const BAD_REQUEST_DATA_TYPE: &str = error::BAD_REQUEST_DATA;
const RESOURCE_NOT_FOUND_TYPE: &str = error::RESOURCE_NOT_FOUND;

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

fn jsonld_link_value() -> String {
    format!("<{TEST_SUITE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

fn vehicle_entity(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": 50
        },
        "@context": TEST_SUITE_CONTEXT
    })
}

fn vehicle_entity_with_speed(id: &str, speed: i64) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "value": speed
        },
        "@context": TEST_SUITE_CONTEXT
    })
}

async fn clean_database() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("failed to connect to database for cleanup");
    sqlx::query(TRUNCATE_ALL).execute(&pool).await.expect("failed to truncate tables");
    pool.close().await;
}

async fn create_entity(client: &Client, entity: &Value) -> (StatusCode, Option<String>) {
    let resp = client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(entity)
        .send()
        .await
        .expect("create_entity request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

async fn delete_entity(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/entities/{}", encode(id))).send().await;
}

async fn create_entity_map_from_get(client: &Client, query: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entityMaps{sep}{query}"))
        .header("Link", jsonld_link_value())
        .send()
        .await
        .expect("create_entity_map_from_get request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn create_entity_map_from_post(client: &Client, body: &Value) -> (StatusCode, HeaderMap, Value) {
    let resp = client
        .post(format!("{BASE_URL}/entityMaps"))
        .header("Content-Type", "application/json")
        .header("Link", jsonld_link_value())
        .json(body)
        .send()
        .await
        .expect("create_entity_map_from_post request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn create_temporal_entity_map_from_get(client: &Client, query: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/temporal/entityMaps{sep}{query}"))
        .header("Link", jsonld_link_value())
        .send()
        .await
        .expect("create_temporal_entity_map_from_get request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn create_temporal_entity_map_from_post(client: &Client, body: &Value) -> (StatusCode, HeaderMap, Value) {
    let resp = client
        .post(format!("{BASE_URL}/temporal/entityMaps"))
        .header("Content-Type", "application/json")
        .header("Link", jsonld_link_value())
        .json(body)
        .send()
        .await
        .expect("create_temporal_entity_map_from_post request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn get_entity_map(client: &Client, id: &str, accept: &str) -> (StatusCode, HeaderMap, Value) {
    let resp = client
        .get(format!("{BASE_URL}/entityMaps/{}", encode(id)))
        .header("Accept", accept)
        .header("Link", jsonld_link_value())
        .send()
        .await
        .expect("get_entity_map request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn update_entity_map(client: &Client, id: &str, patch: &Value) -> (StatusCode, Value) {
    let resp = client
        .patch(format!("{BASE_URL}/entityMaps/{}", encode(id)))
        .header("Content-Type", "application/json")
        .json(patch)
        .send()
        .await
        .expect("update_entity_map request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn delete_entity_map(client: &Client, id: &str) -> StatusCode {
    client
        .delete(format!("{BASE_URL}/entityMaps/{}", encode(id)))
        .send()
        .await
        .expect("delete_entity_map request failed")
        .status()
}

async fn query_entities(client: &Client, query: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities{sep}{query}"))
        .header("Link", jsonld_link_value())
        .send()
        .await
        .expect("query_entities request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn query_entities_with_entity_map(client: &Client, query: &str, entity_map_id: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities{sep}{query}"))
        .header("Link", jsonld_link_value())
        .header("NGSILD-EntityMap", entity_map_id)
        .send()
        .await
        .expect("query_entities_with_entity_map request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn cleanup_entity_map(client: &Client, id: &str) {
    let _ = delete_entity_map(client, id).await;
}

async fn setup_single_vehicle(client: &Client, id: &str) {
    let (status, _) = create_entity(client, &vehicle_entity(id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
#[serial]
async fn entity_map_retrieve_existing_map_returns_200() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-retr");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = body["id"].as_str().expect("map should have id").to_owned();
    pretty_assertions::assert_eq!(body["type"], "EntityMap");
    assert!(body["expiresAt"].is_string());
    assert!(body["entityMap"].is_object());
    assert!(body["linkedMaps"].is_object());

    let entity_map = body["entityMap"].as_object().expect("entityMap should be object");
    for brokers in entity_map.values() {
        let arr = brokers.as_array().expect("entity map value should be array");
        pretty_assertions::assert_eq!(arr.len(), 1);
        pretty_assertions::assert_eq!(arr[0], "@none");
    }
    assert!(body["linkedMaps"].as_object().expect("linkedMaps should be object").is_empty());

    let (status, _, body) = get_entity_map(&c, &map_id, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["id"], map_id);
    pretty_assertions::assert_eq!(body["type"], "EntityMap");

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_retrieve_nonexistent_map_returns_404() {
    clean_database().await;
    let c = client();

    let (status, _, body) = get_entity_map(&c, "urn:ngsi-ld:EntityMap:nonexistent", "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE);
}

#[tokio::test]
#[serial]
async fn entity_map_retrieve_invalid_map_id_returns_400() {
    clean_database().await;
    let c = client();

    let (status, _, body) = get_entity_map(&c, "not-a-valid-uri", "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);
}

#[tokio::test]
#[serial]
async fn entity_map_update_valid_expires_at_returns_204() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-upd");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let new_expires = (Utc::now() + Duration::hours(2)).to_rfc3339();
    let patch = json!({ "expiresAt": new_expires });
    let (status, _) = update_entity_map(&c, &map_id, &patch).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, _) = get_entity_map(&c, &map_id, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_update_output_only_fields_ignored_returns_204() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-upd-oof");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let patch = json!({
        "expiresAt": (Utc::now() + Duration::hours(3)).to_rfc3339(),
        "entityMap": { "urn:fake": ["@none"] },
        "linkedMaps": { "fake": "fake" }
    });
    let (status, _) = update_entity_map(&c, &map_id, &patch).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_update_nonexistent_map_returns_404() {
    clean_database().await;
    let c = client();

    let patch = json!({ "expiresAt": (Utc::now() + Duration::hours(1)).to_rfc3339() });
    let (status, body) = update_entity_map(&c, "urn:ngsi-ld:EntityMap:nonexistent", &patch).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE);
}

#[tokio::test]
#[serial]
async fn entity_map_update_invalid_expires_at_returns_400() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-upd-bad");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let patch = json!({ "expiresAt": "not-a-date" });
    let (status, body) = update_entity_map(&c, &map_id, &patch).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_update_past_expires_at_returns_400() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-upd-past");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let patch = json!({ "expiresAt": (Utc::now() - Duration::minutes(1)).to_rfc3339() });
    let (status, body) = update_entity_map(&c, &map_id, &patch).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_delete_existing_map_then_retrieve_returns_404() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-del");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let status = delete_entity_map(&c, &map_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, body) = get_entity_map(&c, &map_id, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE);

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_delete_nonexistent_map_returns_404() {
    clean_database().await;
    let c = client();

    let status = delete_entity_map(&c, "urn:ngsi-ld:EntityMap:nonexistent").await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn entity_map_create_from_get_by_type_returns_201_with_headers() {
    clean_database().await;
    let c = client();
    let id1 = unique_id("urn:ngsi-ld:Vehicle:em-create1");
    let id2 = unique_id("urn:ngsi-ld:Vehicle:em-create2");

    setup_single_vehicle(&c, &id1).await;
    setup_single_vehicle(&c, &id2).await;

    let (status, headers, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    pretty_assertions::assert_eq!(body["type"], "EntityMap");
    assert!(body["expiresAt"].is_string());

    let entity_map = body["entityMap"].as_object().expect("entityMap should be object");
    pretty_assertions::assert_eq!(entity_map.len(), 2);
    assert!(entity_map.contains_key(&id1));
    assert!(entity_map.contains_key(&id2));

    let location = headers
        .get("location")
        .expect("location header should be present")
        .to_str()
        .expect("location must be ASCII");
    assert!(location.contains("entityMaps/"));
    assert!(headers.get("NGSILD-EntityMap").is_some());

    let (status, _, _) = get_entity_map(&c, &map_id, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id1).await;
    delete_entity(&c, &id2).await;
}

#[tokio::test]
#[serial]
async fn entity_map_create_from_get_with_q_filter_returns_only_matching_ids() {
    clean_database().await;
    let c = client();
    let id1 = unique_id("urn:ngsi-ld:Vehicle:em-q1");
    let id2 = unique_id("urn:ngsi-ld:Vehicle:em-q2");

    let (status, _) = create_entity(&c, &vehicle_entity_with_speed(&id1, 100)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let (status, _) = create_entity(&c, &vehicle_entity_with_speed(&id2, 30)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle&q=speed%3E50").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    let entity_map = body["entityMap"].as_object().expect("entityMap should be object");
    pretty_assertions::assert_eq!(entity_map.len(), 1);
    assert!(entity_map.contains_key(&id1));

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id1).await;
    delete_entity(&c, &id2).await;
}

#[tokio::test]
#[serial]
async fn entity_map_create_from_get_with_lifetime_pt5m_sets_expires_at_in_range() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-lt");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle&entityMapLifetime=PT5M").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    let expires_at = body["expiresAt"].as_str().expect("expiresAt should exist");
    let parsed = chrono::DateTime::parse_from_rfc3339(expires_at).expect("expiresAt should be RFC3339");
    let diff = parsed.signed_duration_since(Utc::now());
    assert!(diff.num_seconds() > 240 && diff.num_seconds() <= 300);

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_create_from_get_without_filters_returns_400() {
    clean_database().await;
    let c = client();

    let (status, _, body) = create_entity_map_from_get(&c, "local=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);
}

#[tokio::test]
#[serial]
async fn entity_map_create_from_get_with_invalid_lifetime_returns_400() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-lt-invalid");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle&entityMapLifetime=NOT_A_DURATION").await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_create_from_post_with_entities_filter_returns_201() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-post");
    setup_single_vehicle(&c, &id).await;

    let body = json!({
        "entities": [{ "type": "Vehicle" }]
    });
    let (status, _, body) = create_entity_map_from_post(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    pretty_assertions::assert_eq!(body["type"], "EntityMap");

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_create_from_post_without_filters_returns_400() {
    clean_database().await;
    let c = client();

    let body = json!({ "local": true });
    let (status, _, body) = create_entity_map_from_post(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);
}

#[tokio::test]
#[serial]
async fn entity_map_create_from_get_with_empty_result_returns_201_empty_entity_map() {
    clean_database().await;
    let c = client();

    let (status, _, body) = create_entity_map_from_get(&c, "type=NonExistentType12345").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    let entity_map = body["entityMap"].as_object().expect("entityMap should be object");
    assert!(entity_map.is_empty());

    cleanup_entity_map(&c, &map_id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_create_from_get_with_many_entities_returns_full_entity_map() {
    clean_database().await;
    let c = client();
    let mut ids = Vec::new();

    for index in 0..50 {
        let id = unique_id(&format!("urn:ngsi-ld:Vehicle:em-many-{index}"));
        setup_single_vehicle(&c, &id).await;
        ids.push(id);
    }

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    let entity_map = body["entityMap"].as_object().expect("entityMap should be object");
    pretty_assertions::assert_eq!(entity_map.len(), 50);

    cleanup_entity_map(&c, &map_id).await;
    for id in ids {
        delete_entity(&c, &id).await;
    }
}

#[tokio::test]
#[serial]
async fn entity_map_temporal_create_from_get_returns_201() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-temp");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_temporal_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    pretty_assertions::assert_eq!(body["type"], "EntityMap");

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_temporal_create_from_post_returns_201() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-temp-post");
    setup_single_vehicle(&c, &id).await;

    let body = json!({
        "entities": [{ "type": "Vehicle" }]
    });
    let (status, _, body) = create_temporal_entity_map_from_post(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = body["id"].as_str().expect("map id missing").to_owned();
    pretty_assertions::assert_eq!(body["type"], "EntityMap");

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_query_entities_with_entity_map_true_returns_201_and_header() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-int");
    setup_single_vehicle(&c, &id).await;

    let (status, headers, _) = query_entities(&c, "type=Vehicle&entityMap=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let map_id = headers
        .get("NGSILD-EntityMap")
        .expect("NGSILD-EntityMap header should be present")
        .to_str()
        .expect("header should be valid ASCII")
        .to_owned();

    let (status, _, body) = get_entity_map(&c, &map_id, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let entity_map = body["entityMap"].as_object().expect("entityMap should be object");
    assert!(entity_map.contains_key(&id));

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_query_entities_without_entity_map_returns_200_without_header() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-int-false");
    setup_single_vehicle(&c, &id).await;

    let (status, headers, _) = query_entities(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(headers.get("NGSILD-EntityMap").is_none());

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_pagination_query_with_frozen_map_covers_all_ids_without_duplicates() {
    clean_database().await;
    let c = client();
    let mut created_ids = Vec::new();

    for index in 0..10 {
        let id = unique_id(&format!("urn:ngsi-ld:Vehicle:em-pg-{index}"));
        setup_single_vehicle(&c, &id).await;
        created_ids.push(id);
    }

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let entity_map = body["entityMap"].as_object().expect("entityMap should be object");
    pretty_assertions::assert_eq!(entity_map.len(), 10);

    let page_queries = [
        "type=Vehicle&limit=3&offset=0",
        "type=Vehicle&limit=3&offset=3",
        "type=Vehicle&limit=3&offset=6",
        "type=Vehicle&limit=3&offset=9",
    ];

    let mut collected_ids = Vec::new();

    for (page_index, query) in page_queries.iter().enumerate() {
        let (status, headers, body) = query_entities_with_entity_map(&c, query, &map_id).await;
        pretty_assertions::assert_eq!(status, StatusCode::OK);

        assert!(headers.get("NGSILD-EntityMap").is_some());

        let arr = body.as_array().expect("query result should be array");
        if page_index < 3 {
            pretty_assertions::assert_eq!(arr.len(), 3);
        } else {
            pretty_assertions::assert_eq!(arr.len(), 1);
        }

        for item in arr {
            let entity_id = item["id"].as_str().expect("entity should have id").to_owned();
            collected_ids.push(entity_id);
        }
    }

    pretty_assertions::assert_eq!(collected_ids.len(), 10);
    let mut dedup = std::collections::HashSet::new();
    for entity_id in &collected_ids {
        dedup.insert(entity_id);
    }
    pretty_assertions::assert_eq!(dedup.len(), 10);

    cleanup_entity_map(&c, &map_id).await;
    for id in created_ids {
        delete_entity(&c, &id).await;
    }
}

#[tokio::test]
#[serial]
async fn entity_map_pagination_new_entity_created_after_map_is_not_in_results() {
    clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Vehicle:em-new1");
    setup_single_vehicle(&c, &id1).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let id2 = unique_id("urn:ngsi-ld:Vehicle:em-new2");
    setup_single_vehicle(&c, &id2).await;

    let (status, _, body) = query_entities_with_entity_map(&c, "type=Vehicle&limit=100", &map_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("query result should be array");
    let ids: Vec<&str> = arr.iter().filter_map(|item| item["id"].as_str()).collect();

    assert!(ids.contains(&id1.as_str()));
    assert!(!ids.contains(&id2.as_str()));

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id1).await;
    delete_entity(&c, &id2).await;
}

#[tokio::test]
#[serial]
async fn entity_map_pagination_deleted_entity_is_skipped_from_results() {
    clean_database().await;
    let c = client();

    let id1 = unique_id("urn:ngsi-ld:Vehicle:em-dsk1");
    let id2 = unique_id("urn:ngsi-ld:Vehicle:em-dsk2");
    setup_single_vehicle(&c, &id1).await;
    setup_single_vehicle(&c, &id2).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let entity_map = body["entityMap"].as_object().expect("entityMap should be object");
    pretty_assertions::assert_eq!(entity_map.len(), 2);

    let status = client()
        .delete(format!("{BASE_URL}/entities/{}", encode(&id1)))
        .send()
        .await
        .expect("delete_entity request failed")
        .status();
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, body) = query_entities_with_entity_map(&c, "type=Vehicle&limit=100", &map_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("query result should be array");
    pretty_assertions::assert_eq!(arr.len(), 1);
    pretty_assertions::assert_eq!(arr[0]["id"], id2);

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id2).await;
}

#[tokio::test]
#[serial]
async fn entity_map_pagination_query_with_count_returns_results_count_header() {
    clean_database().await;
    let c = client();
    let mut ids = Vec::new();

    for index in 0..5 {
        let id = unique_id(&format!("urn:ngsi-ld:Vehicle:em-cnt-{index}"));
        setup_single_vehicle(&c, &id).await;
        ids.push(id);
    }

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let (status, headers, _) = query_entities_with_entity_map(&c, "type=Vehicle&count=true&limit=2", &map_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let count_header = headers.get("NGSILD-Results-Count").expect("count header should be present");
    let count = count_header
        .to_str()
        .expect("header should be ASCII")
        .parse::<usize>()
        .expect("count should be numeric");
    pretty_assertions::assert_eq!(count, 5);

    cleanup_entity_map(&c, &map_id).await;
    for id in ids {
        delete_entity(&c, &id).await;
    }
}

#[tokio::test]
#[serial]
async fn entity_map_content_negotiation_accept_application_json_returns_link_header_without_context_body() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-cn-json");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let (status, headers, body) = get_entity_map(&c, &map_id, "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(headers.get("Link").is_some());
    assert!(body.get("@context").is_none());

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_content_negotiation_accept_application_ld_json_returns_context_in_body() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-cn-ldjson");
    setup_single_vehicle(&c, &id).await;

    let (status, _, body) = create_entity_map_from_get(&c, "type=Vehicle").await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let map_id = body["id"].as_str().expect("map id missing").to_owned();

    let (status, _, body) = get_entity_map(&c, &map_id, "application/ld+json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("@context").is_some());

    cleanup_entity_map(&c, &map_id).await;
    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn entity_map_api_rejects_unknown_query_parameter_with_400() {
    clean_database().await;
    let c = client();

    let resp = c
        .request(Method::GET, format!("{BASE_URL}/entityMaps?type=Vehicle&invalidParam=invalidValue"))
        .header("Link", jsonld_link_value())
        .send()
        .await
        .expect("request failed");

    pretty_assertions::assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: Value = resp.json().await.unwrap_or_default();
    pretty_assertions::assert_eq!(body["type"], INVALID_REQUEST_TYPE);
}

#[tokio::test]
#[serial]
async fn entity_map_create_entity_using_core_context_succeeds_for_baseline() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:em-baseline");

    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "@context": CORE_CONTEXT
    });

    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    delete_entity(&c, &id).await;
}
