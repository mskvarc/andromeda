//! Altitude through the temporal API (NGSI-LD 4.5.7, 5.6.11, 5.7.3, 5.7.4).
//!
//! Temporal instances store the GeoJSON value verbatim just as the current
//! representation does, and a temporal geo-query filters on the same 2D
//! footprint as the non-temporal one.

use crate::fixtures::{assert_coordinates_eq, encode_json, p_2d, p0};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// A temporal Building whose `location` has a single instance at `altitude`.
fn temporal_building(id: &str, altitude: f64, observed_at: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "location": [{
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.4, 52.52, altitude]},
            "observedAt": observed_at
        }],
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Creates a temporal entity and asserts it was accepted.
async fn create_temporal(client: &TestClient, entity: &Value) {
    let response = client.create_temporal_entity(entity).await.expect("create temporal entity failed");
    assert!(
        response.status == StatusCode::CREATED || response.status == StatusCode::NO_CONTENT,
        "temporal create must succeed, got {}",
        response.status
    );
}

/// Retrieves the full temporal representation of an entity.
async fn get_temporal(client: &TestClient, id: &str, query: &str) -> (StatusCode, Value) {
    let response = client
        .get_temporal_entity(id, query, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("temporal retrieve failed");
    let body = response.json_value().expect("temporal body must be valid JSON");
    (response.status, body)
}

/// Collects the entity identifiers of a temporal query result.
fn temporal_ids(body: &Value) -> Vec<String> {
    body.as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|entity| entity.get("id").and_then(Value::as_str).map(String::from))
        .collect()
}

#[tokio::test]
async fn temporal_3d_location_round_trips() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:temporal3d");

    create_temporal(client, &temporal_building(&id, 300.0, "2020-08-01T12:03:00Z")).await;

    let (status, body) = get_temporal(client, &id, "").await;
    assert_eq!(status, StatusCode::OK);

    let instances = body["location"].as_array().map(Vec::as_slice).unwrap_or_default();
    assert!(!instances.is_empty(), "location must have at least one temporal instance");
    let coordinates = instances[0].pointer("/value/coordinates").expect("instance must carry value.coordinates");
    assert_coordinates_eq(coordinates, &json!([13.4, 52.52, 300.0]));

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_values_representation_keeps_altitude() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:temporalvalues");

    create_temporal(client, &temporal_building(&id, 300.0, "2020-08-01T12:03:00Z")).await;

    let second = json!({
        "location": [{
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.4, 52.52, 350.0]},
            "observedAt": "2020-08-01T12:05:00Z"
        }],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let append = client.append_entity_attrs(&id, &second).await.expect("temporal append failed");
    assert!(
        append.status == StatusCode::NO_CONTENT || append.status == StatusCode::MULTI_STATUS,
        "appending a second instance must succeed, got {}",
        append.status
    );

    // NGSI-LD 4.5.7: temporalValues carries [value, timestamp] pairs, and the
    // value half must keep all three ordinates.
    let (status, body) = get_temporal(client, &id, "options=temporalValues&timerel=after&timeAt=2020-01-01T00:00:00Z").await;
    assert_eq!(status, StatusCode::OK);

    let values = body
        .pointer("/location/values")
        .and_then(Value::as_array)
        .expect("temporalValues representation must expose location.values");
    assert!(!values.is_empty(), "at least one temporal value must be present");
    for entry in values {
        let coordinates = entry
            .get(0)
            .and_then(|value| value.get("coordinates"))
            .and_then(Value::as_array)
            .expect("each temporalValues entry must carry GeoJSON coordinates");
        assert_eq!(coordinates.len(), 3, "temporalValues must preserve the altitude ordinate");
    }

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_georel_near_ignores_altitude() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let ground = scope.id("Building:temporal_near_ground");
    let elevated = scope.id("Building:temporal_near_elevated");

    create_temporal(client, &temporal_building(&ground, 0.0, "2020-08-01T12:03:00Z")).await;
    create_temporal(client, &temporal_building(&elevated, 500.0, "2020-08-01T12:03:00Z")).await;

    let coordinates = encode_json(&p0());
    let query_string = format!("type=Building&timerel=after&timeAt=2020-01-01T00:00:00Z&georel=near;maxDistance==100&geometry=Point&coordinates={coordinates}");
    let response = client
        .query_temporal_entities(&query_string, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("temporal query failed");
    assert_eq!(response.status, StatusCode::OK);

    let matched = temporal_ids(&response.json_value().expect("temporal query body must be valid JSON"));
    assert!(matched.contains(&ground), "ground history must match a 3D near reference");
    assert!(matched.contains(&elevated), "elevated history must match a 3D near reference");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_query_georel_2d_reference_matches_3d_history() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let ground = scope.id("Building:temporal_2dref_ground");
    let elevated = scope.id("Building:temporal_2dref_elevated");

    create_temporal(client, &temporal_building(&ground, 0.0, "2020-08-01T12:03:00Z")).await;
    create_temporal(client, &temporal_building(&elevated, 500.0, "2020-08-01T12:03:00Z")).await;

    let coordinates = encode_json(&p_2d());
    let query_string = format!("type=Building&timerel=after&timeAt=2020-01-01T00:00:00Z&georel=near;maxDistance==100&geometry=Point&coordinates={coordinates}");
    let response = client
        .query_temporal_entities(&query_string, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("temporal query failed");
    assert_eq!(response.status, StatusCode::OK);

    let matched = temporal_ids(&response.json_value().expect("temporal query body must be valid JSON"));
    assert!(matched.contains(&ground), "a 2D reference must match a 2D-equivalent history");
    assert!(matched.contains(&elevated), "a 2D reference must match a 3D history on the same footprint");

    scope.finish().await.expect("cleanup failed");
}

/// NGSI-LD 4.10 applies to a temporal query exactly as it does to a
/// current-state one: `geoproperty` may name any GeoProperty, and one that is
/// not the default is read from its `current_attribute_values` instance rows,
/// correlated through the alias stage 1 joins `entities` under.
#[tokio::test]
async fn temporal_query_georel_on_a_custom_geoproperty_matches() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let near = scope.id("Building:temporal_geoprop_near");
    let far = scope.id("Building:temporal_geoprop_far");

    // Both `location` values are swapped relative to `observationPoint`, so a
    // query that fell back to the default GeoProperty would return the wrong
    // one of the two entities rather than none.
    create_temporal(client, &temporal_building_with_observation_point(&near, [30.0, 10.0], [13.4, 52.52, 800.0])).await;
    create_temporal(client, &temporal_building_with_observation_point(&far, [13.4, 52.52], [30.0, 10.0, 0.0])).await;

    let coordinates = encode_json(&p0());
    let query_string = format!(
        "type=Building&timerel=after&timeAt=2020-01-01T00:00:00Z&georel=near;maxDistance==100&geometry=Point&coordinates={coordinates}&geoproperty=observationPoint"
    );
    let response = client
        .query_temporal_entities(&query_string, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("temporal query failed");
    assert_eq!(response.status, StatusCode::OK);

    let matched = temporal_ids(&response.json_value().expect("temporal query body must be valid JSON"));
    assert!(matched.contains(&near), "the entity whose observationPoint is near the reference must match");
    assert!(
        !matched.contains(&far),
        "an entity whose observationPoint is distant must not be admitted by its location"
    );

    scope.finish().await.expect("cleanup failed");
}

/// A temporal `Building` carrying both a `location` and an `observationPoint`,
/// each with one observed instance.
fn temporal_building_with_observation_point(id: &str, location: [f64; 2], observation_point: [f64; 3]) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "location": [{
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": location},
            "observedAt": "2020-08-01T12:03:00Z"
        }],
        "observationPoint": [{
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": observation_point},
            "observedAt": "2020-08-01T12:03:00Z"
        }],
        "@context": [TEST_SUITE_CONTEXT]
    })
}
