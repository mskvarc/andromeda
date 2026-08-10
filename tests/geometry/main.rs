//! GeoProperty geometry rendering integration tests (NGSI-LD 6.3.3, 6.4.3.2).
//!
//! Naming convention: `geometry_<area>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

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

async fn get_entity_geojson(client: &TestClient, id: &str, query: &str) -> Value {
    let response = client
        .get_entity(id, query, "application/geo+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("get_entity_geojson failed");
    assert_eq!(response.status, StatusCode::OK);
    response.json_value().expect("response body must be valid JSON")
}

async fn query_entities_geojson(client: &TestClient, query: &str) -> Value {
    let response = client
        .query_entities(query, "application/geo+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities_geojson failed");
    assert_eq!(response.status, StatusCode::OK);
    response.json_value().expect("response body must be valid JSON")
}

fn feature_for<'a>(features: &'a [Value], id: &str) -> &'a Value {
    features
        .iter()
        .find(|feature| feature["id"].as_str() == Some(id))
        .expect("feature for expected id must be present")
}

// --- Collection GeoJSON rendering (spec 6.4.3.2) ---

#[tokio::test]
async fn geometry_collection_custom_property_uses_specified_attribute() {
    let scope = TestEnvironment::from_suite("geometry").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:gpc1");
    create(
        client,
        &json!({
            "id": id,
            "type": "Building",
            "observationPoint": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0.0, 0.0]}},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let body = query_entities_geojson(
        client,
        &format!("id={}&type=Building&geometryProperty=observationPoint", urlencoding::encode(&id)),
    )
    .await;
    assert_eq!(body["type"], "FeatureCollection");
    let features = body["features"].as_array().expect("features must be an array");
    let feature = feature_for(features, &id);
    assert_eq!(feature["geometry"]["type"], "Point");
    let lon = feature["geometry"]["coordinates"][0].as_f64().unwrap();
    assert!(
        (lon - 13.3986).abs() < 0.001,
        "geometryProperty=observationPoint should use observationPoint coordinates, got lon={lon}"
    );
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn geometry_collection_absent_geometry_property_defaults_to_location_attribute() {
    let scope = TestEnvironment::from_suite("geometry").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:gpc2");
    create(
        client,
        &json!({
            "id": id,
            "type": "Building",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.3522, 48.8566]}},
            "otherGeo": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [99.0, 99.0]}},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let body = query_entities_geojson(client, &format!("id={}&type=Building", urlencoding::encode(&id))).await;
    let features = body["features"].as_array().expect("features must be an array");
    let feature = feature_for(features, &id);
    let lon = feature["geometry"]["coordinates"][0].as_f64().unwrap();
    assert!((lon - 2.3522).abs() < 0.001, "absent geometryProperty must use `location`, got lon={lon}");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn geometry_collection_entity_with_no_location_has_null_geometry_in_feature() {
    let scope = TestEnvironment::from_suite("geometry").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:gpc3");
    create(
        client,
        &json!({
            "id": id,
            "type": "Building",
            "temperature": {"type": "Property", "value": 22.0},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let body = query_entities_geojson(client, &format!("id={}&type=Building", urlencoding::encode(&id))).await;
    assert_eq!(body["type"], "FeatureCollection");
    let features = body["features"].as_array().expect("features must be an array");
    let feature = feature_for(features, &id);
    assert!(feature["geometry"].is_null(), "entity without a GeoProperty must render with null geometry");
    scope.finish().await.expect("cleanup failed");
}

// --- Single-entity GeoJSON rendering (spec 6.3.3) ---

#[tokio::test]
async fn geometry_entity_custom_geometry_property_uses_specified_attribute() {
    let scope = TestEnvironment::from_suite("geometry").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:gpe1");
    create(
        client,
        &json!({
            "id": id,
            "type": "Building",
            "observationPoint": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0.0, 0.0]}},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let body = get_entity_geojson(client, &id, "geometryProperty=observationPoint").await;
    assert_eq!(body["type"], "Feature");
    assert_eq!(body["geometry"]["type"], "Point");
    let lon = body["geometry"]["coordinates"][0].as_f64().unwrap();
    assert!((lon - 13.3986).abs() < 0.001, "geometryProperty=observationPoint mismatch, got lon={lon}");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn geometry_entity_absent_geometry_property_defaults_to_location_attribute() {
    let scope = TestEnvironment::from_suite("geometry").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:gpe2");
    create(
        client,
        &json!({
            "id": id,
            "type": "Building",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [4.9041, 52.3676]}},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let body = get_entity_geojson(client, &id, "").await;
    assert_eq!(body["type"], "Feature");
    let lon = body["geometry"]["coordinates"][0].as_f64().unwrap();
    assert!(
        (lon - 4.9041).abs() < 0.001,
        "absent geometryProperty must default to `location`, got lon={lon}"
    );
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn geometry_entity_without_geo_attribute_returns_feature_with_null_geometry() {
    let scope = TestEnvironment::from_suite("geometry").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:gpe3");
    create(
        client,
        &json!({
            "id": id,
            "type": "Building",
            "temperature": {"type": "Property", "value": 20.0},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;

    let body = get_entity_geojson(client, &id, "").await;
    assert_eq!(body["type"], "Feature");
    assert!(body["geometry"].is_null(), "entity without GeoProperty must render with null geometry");
    scope.finish().await.expect("cleanup failed");
}
