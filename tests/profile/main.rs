//! Profile link header integration tests (RFC 8288 Section 3.4, NGSI-LD Clause 6.3.21).
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly.
//! Naming convention: `profile_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";

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

fn jsonld_link_with_profile(profile_uri: &str) -> String {
    format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"; profile=\"{profile_uri}\"")
}

fn jsonld_link_value() -> String {
    format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

async fn clean_database() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("failed to connect to database for cleanup");
    sqlx::query(TRUNCATE_ALL).execute(&pool).await.expect("failed to truncate tables");
    pool.close().await;
}

async fn create_entity_with_link(client: &Client, body: &Value, link_header: &str) -> (StatusCode, Value) {
    let resp = client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/json")
        .header("Link", link_header)
        .json(body)
        .send()
        .await
        .expect("create_entity request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn get_entity(client: &Client, id: &str) -> (StatusCode, Value) {
    let resp = client
        .get(format!("{BASE_URL}/entities/{}", urlencoding::encode(id)))
        .header("Accept", "application/json")
        .header("Link", jsonld_link_value())
        .send()
        .await
        .expect("get_entity request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn delete_entity(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/entities/{}", urlencoding::encode(id))).send().await;
}

/// Profile parameter in Link header accepted on entity creation.
#[tokio::test]
#[serial]
async fn profile_link_header_accepted_on_entity_creation() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Testing:profile");
    let entity = json!({
        "id": &id,
        "type": "Testing",
        "name": {"type": "Property", "value": "test"}
    });

    let link = jsonld_link_with_profile("https://uri.etsi.org/ngsi-ld/profile/version");
    let (status, _) = create_entity_with_link(&c, &entity, &link).await;
    assert_eq!(status, StatusCode::CREATED, "entity creation with profiled Link should succeed");

    delete_entity(&c, &id).await;
}

/// Entity created with profiled Link header is retrievable.
#[tokio::test]
#[serial]
async fn profile_link_header_preserved_in_context_extraction() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Testing:profile-retrieve");
    let entity = json!({
        "id": &id,
        "type": "Testing",
        "name": {"type": "Property", "value": "retrieve-test"}
    });

    let link = jsonld_link_with_profile("https://uri.etsi.org/ngsi-ld/profile/version");
    let (status, _) = create_entity_with_link(&c, &entity, &link).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id).await;
    assert_eq!(status, StatusCode::OK, "entity should be retrievable after creation with profiled Link");
    assert_eq!(body["id"], id);

    delete_entity(&c, &id).await;
}

/// Link header with both context rel and profile param coexist correctly.
#[tokio::test]
#[serial]
async fn profile_link_header_with_standard_context_coexist() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Testing:profile-coexist");
    let entity = json!({
        "id": &id,
        "type": "Testing",
        "name": {"type": "Property", "value": "coexist-test"}
    });

    // Link with both context rel and profile
    let link = jsonld_link_with_profile("https://uri.etsi.org/ngsi-ld/profile/version");
    let (status, _) = create_entity_with_link(&c, &entity, &link).await;
    assert_eq!(status, StatusCode::CREATED, "Link with context and profile should work together");

    // Verify entity is correctly created and retrievable
    let (status, body) = get_entity(&c, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "Testing");

    delete_entity(&c, &id).await;
}

/// Link header with empty profile value returns error.
#[tokio::test]
async fn profile_link_header_missing_value_returns_error() {
    let c = client();
    let id = unique_id("urn:ngsi-ld:Testing:profile-empty");
    let entity = json!({
        "id": &id,
        "type": "Testing",
        "name": {"type": "Property", "value": "bad-profile"}
    });

    // Malformed Link with empty profile=
    let link = format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"; profile=");
    let (status, _) = create_entity_with_link(&c, &entity, &link).await;
    assert!(
        status == StatusCode::BAD_REQUEST || status.is_client_error(),
        "empty profile value should cause client error, got {status}"
    );
}
