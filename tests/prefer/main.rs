//! `Prefer` / `Preference-Applied` header integration tests for the NGSI-LD context broker.
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly -- no dependency
//! on the libtest-mimic harness or its fixtures module.
//!
//! Covers spec clause 6.3.6 (Prefer header handling):
//! - `body=json` / `body=ld+json` GeoJSON body preference
//! - `ngsi-ld=<version>` version conformance negotiation (203 + Preference-Applied)
//! - Version adaptation payload transformations (v1.0 compat)
//!
//! Naming convention: `prefer_<area>_<scenario>_<expected_outcome>`.

// Test infrastructure legitimately uses expect/unwrap to fail fast on transport errors.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use reqwest::{Client, StatusCode, header::HeaderMap};
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

/// GET /entities/{id} with caller-supplied Accept, Prefer, and Link headers.
/// Returns (status, response headers, body).
async fn get_entity(client: &Client, id: &str, accept: &str, prefer: Option<&str>, query: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let mut req = client
        .get(format!("{BASE_URL}/entities/{id}{sep}{query}"))
        .header("Accept", accept)
        .header("Link", link_header());
    if let Some(p) = prefer {
        req = req.header("Prefer", p);
    }
    let resp = req.send().await.expect("get_entity request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

/// GET /entities with caller-supplied Accept, Prefer, and Link headers.
/// Returns (status, response headers, body).
async fn query_entities(client: &Client, accept: &str, prefer: Option<&str>, query: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let mut req = client
        .get(format!("{BASE_URL}/entities{sep}{query}"))
        .header("Accept", accept)
        .header("Link", link_header());
    if let Some(p) = prefer {
        req = req.header("Prefer", p);
    }
    let resp = req.send().await.expect("query_entities request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

// ---------------------------------------------------------------------------
// GeoJSON body preference: body=ld+json vs body=json (spec 6.3.6)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn prefer_geojson_retrieve_default_body_includes_context_inline() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:prefer1");
    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = get_entity(&c, &id, "application/geo+json", None, "geometryProperty=location").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("@context").is_some(), "default GeoJSON response must include @context inline in body");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn prefer_geojson_retrieve_body_ld_json_includes_context_inline() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:prefer2");
    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = get_entity(&c, &id, "application/geo+json", Some("body=ld+json"), "geometryProperty=location").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("@context").is_some(), "Prefer: body=ld+json must include @context inline in body");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn prefer_geojson_retrieve_body_json_moves_context_to_link_header() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:prefer3");
    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, headers, body) = get_entity(&c, &id, "application/geo+json", Some("body=json"), "geometryProperty=location").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body.get("@context").is_none(), "Prefer: body=json must omit @context from body");
    assert!(
        headers.get("link").is_some(),
        "Prefer: body=json must move @context reference to Link response header"
    );

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn prefer_geojson_query_body_json_removes_context_from_collection_body() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:prefer4");
    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [2.3522, 48.8566]}
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, headers, body) = query_entities(&c, "application/geo+json", Some("body=json"), "type=Building").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["type"], "FeatureCollection");
    assert!(
        body.get("@context").is_none(),
        "Prefer: body=json must omit @context from FeatureCollection body"
    );
    assert!(headers.get("link").is_some(), "Prefer: body=json must add Link header with context reference");

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// Version conformance negotiation: ngsi-ld=<version> (spec 6.3.6)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn prefer_version_retrieve_older_version_returns_non_authoritative_with_preference_applied() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:prefer5");
    let entity = json!({
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": 25.0},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, headers, _) = get_entity(&c, &id, "application/ld+json", Some("ngsi-ld=1.3"), "").await;
    pretty_assertions::assert_eq!(
        status,
        StatusCode::NON_AUTHORITATIVE_INFORMATION,
        "version preference for older version must return 203"
    );
    let pref_applied = headers.get("preference-applied").and_then(|v| v.to_str().ok());
    pretty_assertions::assert_eq!(
        pref_applied,
        Some("ngsi-ld=1.9"),
        "Preference-Applied must advertise the broker's implemented version"
    );

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn prefer_version_query_older_version_returns_non_authoritative_with_preference_applied() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:prefer6");
    let entity = json!({
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": 25.0},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, headers, _) = query_entities(&c, "application/ld+json", Some("ngsi-ld=1.4"), "type=Building").await;
    pretty_assertions::assert_eq!(
        status,
        StatusCode::NON_AUTHORITATIVE_INFORMATION,
        "version preference for older version on collection must return 203"
    );
    let pref_applied = headers.get("preference-applied").and_then(|v| v.to_str().ok());
    pretty_assertions::assert_eq!(
        pref_applied,
        Some("ngsi-ld=1.9"),
        "Preference-Applied must advertise the broker's implemented version"
    );

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn prefer_absent_retrieve_returns_ok_without_preference_applied_header() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:prefer7");
    let entity = json!({
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": 25.0},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, headers, _) = get_entity(&c, &id, "application/ld+json", None, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK, "absent Prefer header must return 200");
    assert!(
        headers.get("preference-applied").is_none(),
        "absent Prefer header must not produce Preference-Applied response header"
    );

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn prefer_combined_body_json_and_version_returns_non_authoritative_without_context_in_body() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:prefer8");
    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, headers, body) = get_entity(&c, &id, "application/geo+json", Some("body=json, ngsi-ld=1.3"), "geometryProperty=location").await;
    pretty_assertions::assert_eq!(
        status,
        StatusCode::NON_AUTHORITATIVE_INFORMATION,
        "combined body=json + ngsi-ld=1.3 must return 203"
    );
    assert!(
        body.get("@context").is_none(),
        "body=json preference must still suppress inline @context in 203 response"
    );
    assert!(headers.get("link").is_some(), "body=json preference must add Link header even in 203 response");
    let pref_applied = headers.get("preference-applied").and_then(|v| v.to_str().ok());
    pretty_assertions::assert_eq!(pref_applied, Some("ngsi-ld=1.9"));

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// Version adaptation payload transformations (spec 6.3.6 + NGSI-LD v1.x compat)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn prefer_version_1_0_retrieve_removes_dataset_id_from_attribute() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:prefer9");
    let entity = json!({
        "id": id,
        "type": "Building",
        "temperature": {
            "type": "Property",
            "value": 25.0,
            "datasetId": "urn:ngsi-ld:Dataset:temp1"
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = get_entity(&c, &id, "application/ld+json", Some("ngsi-ld=1.0"), "").await;
    pretty_assertions::assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION);

    // In v1.0 the body uses expanded IRIs; retrieve under the expanded temperature key.
    let temp_key = "https://uri.fiware.org/ns/data-models#temperature";
    let temp = &body[temp_key];
    assert!(temp.get("datasetId").is_none(), "v1.0 adaptation must strip datasetId from Property attributes");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn prefer_version_1_0_retrieve_removes_scope_from_entity() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:prefer10");
    let entity = json!({
        "id": id,
        "type": "Building",
        "scope": ["/A/B"],
        "temperature": {"type": "Property", "value": 25.0},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = get_entity(&c, &id, "application/ld+json", Some("ngsi-ld=1.0"), "").await;
    pretty_assertions::assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION);
    assert!(body.get("scope").is_none(), "v1.0 adaptation must strip the scope field from the entity");

    delete_entity(&c, &id).await;
}
