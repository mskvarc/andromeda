//! Altitude on context source registrations (NGSI-LD 5.9.2, 5.10.2).
//!
//! A registration's `location` is a plain GeoJSON geometry rather than a
//! GeoProperty, and it obeys the same rules: three ordinates round-trip
//! verbatim, and registration discovery filters on the 2D footprint.

use crate::fixtures::{assert_coordinates_eq, encode_json, pillar_ring_3d};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// A registration for `Building` entities, optionally scoped to a location.
fn registration(id: &str, location: Option<Value>) -> Value {
    let mut document = json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Building"}]}],
        "endpoint": "http://127.0.0.1:9999",
        "@context": [TEST_SUITE_CONTEXT]
    });
    if let Some(geometry) = location {
        document["location"] = geometry;
    }
    document
}

/// Collects the registration identifiers present in a discovery result.
fn registration_ids(body: &Value) -> Vec<String> {
    body.as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| entry.get("id").and_then(Value::as_str).map(String::from))
        .collect()
}

#[tokio::test]
async fn csr_3d_location_round_trips() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let registration_id = scope.id("ContextSourceRegistration:location3d");
    let geometry = json!({"type": "Polygon", "coordinates": pillar_ring_3d(120.0)});

    let create = client
        .create_registration(&registration(&registration_id, Some(geometry.clone())))
        .await
        .expect("create registration failed");
    assert_eq!(create.status, StatusCode::CREATED);

    let retrieve = client
        .retrieve_registration(&registration_id, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve registration failed");
    assert_eq!(retrieve.status, StatusCode::OK);

    let body = retrieve.json_value().expect("registration must be valid JSON");
    assert_coordinates_eq(
        body.pointer("/location/coordinates").expect("registration must carry location.coordinates"),
        &pillar_ring_3d(120.0),
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_discovery_geo_filter_footprint() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let covering = scope.id("ContextSourceRegistration:covering3d");
    let distant = scope.id("ContextSourceRegistration:distant2d");
    let unlocated = scope.id("ContextSourceRegistration:unlocated");

    let covering_geometry = json!({"type": "Polygon", "coordinates": pillar_ring_3d(500.0)});
    let distant_geometry = json!({
        "type": "Polygon",
        "coordinates": [[[30.0, 10.0], [31.0, 10.0], [31.0, 11.0], [30.0, 11.0], [30.0, 10.0]]]
    });

    for document in [
        registration(&covering, Some(covering_geometry)),
        registration(&distant, Some(distant_geometry)),
        registration(&unlocated, None),
    ] {
        let response = client.create_registration(&document).await.expect("create registration failed");
        assert_eq!(response.status, StatusCode::CREATED);
    }

    let coordinates = encode_json(&json!([13.4, 52.52]));
    let query_string = format!("type=Building&georel=intersects&geometry=Point&coordinates={coordinates}");
    let response = client
        .query_registrations(&query_string, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("registration discovery failed");
    assert_eq!(response.status, StatusCode::OK);

    let matched = registration_ids(&response.json_value().expect("discovery body must be valid JSON"));
    assert!(matched.contains(&covering), "a 3D registration covering the point footprint must be discovered");
    assert!(!matched.contains(&distant), "a registration whose footprint excludes the point must not match");
    // NGSI-LD 5.10.2: a registration without a location is not excluded by a
    // geo-filter, since it makes no spatial claim.
    assert!(matched.contains(&unlocated), "a registration without a location must remain discoverable");

    scope.finish().await.expect("cleanup failed");
}
