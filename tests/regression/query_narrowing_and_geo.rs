//! A temporal geo query narrows on the `GeoProperty` instances within the
//! window, not on a single hoisted current-state geometry.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.7.4.4 step S3: when a geoquery accompanies a
//! temporal query, "those geospatial restrictions shall be checked against the
//! `GeoProperty` instances that are within the interval defined by the temporal
//! query", and clause 4.10 counts an entity as matching when *any* such instance
//! meets the restriction. Andromeda evaluated the temporal geoquery against
//! current state: for the default `location` it read the single hoisted
//! `entity_search.location` column, which holds one instant -- the first
//! datasetId-less instance -- so an entity whose *other* location instance was
//! the one near the reference was dropped. The restriction is now an existential
//! over the `attribute_history` instances of the target `GeoProperty`, windowed
//! by the same `timerel`/`timeAt` the temporal query applies.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// A temporal Vehicle whose `location` carries two datasetId-less instances: a
/// far one observed first and a near one observed second. The reference point
/// the query measures from is ~335 m from the near instance and ~1200 km from
/// the far one, so only an instance-wise check selects the entity.
fn vehicle_with_two_locations(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "location": [
            {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.35, 42.22]}, "observedAt": "2020-10-01T12:03:00Z"},
            {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}, "observedAt": "2020-10-01T12:05:00Z"}
        ],
        "@context": [CORE_CONTEXT]
    })
}

/// A temporal Vehicle with no `location`, so it can never meet the geoquery.
fn vehicle_without_location(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 10, "observedAt": "2020-10-01T12:04:00Z"}],
        "@context": [CORE_CONTEXT]
    })
}

/// Create a temporal entity and assert it was taken.
async fn create_temporal(client: &TestClient, entity: &Value) {
    let response = client.create_temporal_entity(entity).await.expect("create temporal entity");
    assert!(
        response.status == StatusCode::CREATED || response.status == StatusCode::NO_CONTENT,
        "temporal create: {} {:?}",
        response.status,
        response.body
    );
}

/// The identifiers a temporal query result carries.
fn result_ids(body: &Value) -> Vec<String> {
    body.as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|entity| entity.get("id").and_then(Value::as_str).map(String::from))
        .collect()
}

/// Clause 5.7.4.4 S3 with clause 4.10: a `georel=near` temporal query selects an
/// entity when any of its in-window `location` instances is within the distance,
/// even when a different instance is the one hoisted into current state.
#[tokio::test]
async fn regression_temporal_geo_a_near_query_selects_an_entity_by_a_non_first_location_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let near = scope.id("Vehicle:021-09-near");
    let without = scope.id("Vehicle:021-09-without");

    create_temporal(client, &vehicle_with_two_locations(&near)).await;
    create_temporal(client, &vehicle_without_location(&without)).await;

    let query = "type=Vehicle&timerel=after&timeAt=2020-07-01T12:05:00Z&georel=near;maxDistance==2000&geometry=Point&coordinates=[-8.503,41.202]";
    let response = client.query_temporal_entities(query, Some(CORE_CONTEXT)).await.expect("temporal query");
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let body = response.json_value().expect("a temporal query answers JSON");

    let ids = result_ids(&body);
    assert!(
        ids.contains(&near),
        "the entity whose second location is near the reference is selected: {ids:?}"
    );
    assert!(!ids.contains(&without), "an entity with no location never meets the geoquery: {ids:?}");

    client.delete_temporal_entity(&near).await.expect("delete temporal entity");
    client.delete_temporal_entity(&without).await.expect("delete temporal entity");
    scope.finish().await.expect("cleanup failed");
}

/// The narrowing selects the entity but does not trim its instances: clause
/// 5.7.4.4 selects entities, and the temporal representation still carries every
/// `location` instance in the window, near and far alike.
#[tokio::test]
async fn regression_temporal_geo_the_geoquery_selects_entities_without_trimming_instances() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let near = scope.id("Vehicle:021-09-instances");

    create_temporal(client, &vehicle_with_two_locations(&near)).await;

    let query = "type=Vehicle&timerel=after&timeAt=2020-07-01T12:05:00Z&georel=near;maxDistance==2000&geometry=Point&coordinates=[-8.503,41.202]";
    let response = client.query_temporal_entities(query, Some(CORE_CONTEXT)).await.expect("temporal query");
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let body = response.json_value().expect("a temporal query answers JSON");

    let entity = body
        .as_array()
        .and_then(|entities| entities.iter().find(|entity| entity.get("id").and_then(Value::as_str) == Some(near.as_str())))
        .expect("the selected entity is present");
    let instances = entity["location"].as_array().expect("location carries its instances");
    assert_eq!(instances.len(), 2, "both in-window location instances are returned: {}", entity["location"]);

    client.delete_temporal_entity(&near).await.expect("delete temporal entity");
    scope.finish().await.expect("cleanup failed");
}
