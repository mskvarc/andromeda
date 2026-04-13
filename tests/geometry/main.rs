//! GeoProperty geometry rendering integration tests for the NGSI-LD context broker.
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly -- no dependency
//! on the libtest-mimic harness or its fixtures module.
//!
//! Covers spec section 6.4.3.2 (GET /entities collection with GeoJSON Accept) and
//! 6.3.3 (GET /entities/{entityId} single-entity GeoJSON rendering), specifically
//! the `geometryProperty` query parameter behaviour.
//!
//! Naming convention: `geometry_<area>_<scenario>_<expected_outcome>`.

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

/// GET /entities/{id} with Accept: application/geo+json and Link context.
/// Returns (status, response headers, body).
async fn get_entity_geojson(client: &Client, id: &str, query: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities/{id}{sep}{query}"))
        .header("Accept", "application/geo+json")
        .header("Link", link_header())
        .send()
        .await
        .expect("get_entity_geojson request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

/// GET /entities with Accept: application/geo+json and Link context.
/// Returns (status, response headers, body).
async fn query_entities_geojson(client: &Client, query: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities{sep}{query}"))
        .header("Accept", "application/geo+json")
        .header("Link", link_header())
        .send()
        .await
        .expect("query_entities_geojson request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

// ---------------------------------------------------------------------------
// Collection GeoJSON rendering (spec 6.4.3.2)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn geometry_collection_custom_property_uses_specified_attribute() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:gpc1");
    let entity = json!({
        "id": id,
        "type": "Building",
        "observationPoint": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}
        },
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [0.0, 0.0]}
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = query_entities_geojson(&c, "type=Building&geometryProperty=observationPoint").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["type"], "FeatureCollection");

    let features = body["features"].as_array().expect("features must be an array");
    assert!(!features.is_empty(), "collection must contain at least one feature");

    let geom = &features[0]["geometry"];
    pretty_assertions::assert_eq!(geom["type"], "Point");
    let coords = geom["coordinates"].as_array().expect("coordinates must be an array");
    let lon = coords[0].as_f64().unwrap();
    // geometryProperty=observationPoint: must use (13.3986, 52.5547), not location (0.0, 0.0)
    assert!(
        (lon - 13.3986).abs() < 0.001,
        "geometryProperty=observationPoint should use observationPoint coordinates, got lon={lon}"
    );

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn geometry_collection_absent_geometry_property_defaults_to_location_attribute() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:gpc2");
    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [2.3522, 48.8566]}
        },
        "otherGeo": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [99.0, 99.0]}
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // No geometryProperty param → must fall back to `location`
    let (status, _, body) = query_entities_geojson(&c, "type=Building").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let features = body["features"].as_array().expect("features must be an array");
    assert!(!features.is_empty(), "collection must contain at least one feature");

    let coords = &features[0]["geometry"]["coordinates"];
    let lon = coords[0].as_f64().unwrap();
    assert!(
        (lon - 2.3522).abs() < 0.001,
        "absent geometryProperty must use `location` attribute, got lon={lon}"
    );

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn geometry_collection_entity_with_no_location_has_null_geometry_in_feature() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:gpc3");
    let entity = json!({
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": 22.0},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = query_entities_geojson(&c, "type=Building").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["type"], "FeatureCollection");

    let features = body["features"].as_array().expect("features must be an array");
    let feature = features
        .iter()
        .find(|f| f["id"].as_str().is_some_and(|fid| fid == id))
        .expect("feature for created entity must be present");
    assert!(feature["geometry"].is_null(), "entity without a GeoProperty must render with null geometry");

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// Single-entity GeoJSON rendering (spec 6.3.3)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn geometry_entity_custom_geometry_property_uses_specified_attribute() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:gpe1");
    let entity = json!({
        "id": id,
        "type": "Building",
        "observationPoint": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}
        },
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [0.0, 0.0]}
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = get_entity_geojson(&c, &id, "geometryProperty=observationPoint").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["type"], "Feature");

    let geom = &body["geometry"];
    pretty_assertions::assert_eq!(geom["type"], "Point");
    let coords = geom["coordinates"].as_array().expect("coordinates must be an array");
    let lon = coords[0].as_f64().unwrap();
    assert!(
        (lon - 13.3986).abs() < 0.001,
        "geometryProperty=observationPoint must use observationPoint coords, got lon={lon}"
    );

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn geometry_entity_absent_geometry_property_defaults_to_location_attribute() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:gpe2");
    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [4.9041, 52.3676]}
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = get_entity_geojson(&c, &id, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["type"], "Feature");

    let coords = &body["geometry"]["coordinates"];
    let lon = coords[0].as_f64().unwrap();
    assert!(
        (lon - 4.9041).abs() < 0.001,
        "absent geometryProperty must default to `location`, got lon={lon}"
    );

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn geometry_entity_without_geo_attribute_returns_feature_with_null_geometry() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:gpe3");
    let entity = json!({
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": 20.0},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = get_entity_geojson(&c, &id, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["type"], "Feature");
    assert!(body["geometry"].is_null(), "entity without GeoProperty must render with null geometry");

    delete_entity(&c, &id).await;
}
