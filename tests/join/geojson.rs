//! `join=flat` under `Accept: application/geo+json` (NGSI-LD 6.3.8, 6.4.3.2).

use crate::create;
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// A Building with two GeoProperties, so `geometryProperty` has a choice to
/// make (clause 6.3.8).
fn located_building(id: &str, longitude: f64) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0.0, 0.0]}},
        "observationPoint": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [longitude, 52.5547]}},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// A Building with two GeoProperties and a Relationship to `target`.
fn located_linked_building(id: &str, longitude: f64, target: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0.0, 0.0]}},
        "observationPoint": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [longitude, 52.5547]}},
        "linkedTo": {"type": "Relationship", "object": target},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

async fn query_geojson(client: &TestClient, query: &str) -> Value {
    let response = client
        .query_entities(query, "application/geo+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities failed");
    assert_eq!(response.status, StatusCode::OK);
    response.json_value().expect("response body must be valid JSON")
}

async fn get_geojson(client: &TestClient, id: &str, query: &str) -> Value {
    let response = client
        .get_entity(id, query, "application/geo+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("get_entity failed");
    assert_eq!(response.status, StatusCode::OK);
    response.json_value().expect("response body must be valid JSON")
}

fn features(body: &Value) -> &Vec<Value> {
    let features = body["features"].as_array();
    assert!(features.is_some(), "features must be an array: {body}");
    features.expect("the features member was just asserted to be an array")
}

fn feature_ids(body: &Value) -> Vec<String> {
    features(body).iter().filter_map(|feature| feature["id"].as_str().map(str::to_owned)).collect()
}

fn feature_for<'a>(body: &'a Value, id: &str) -> &'a Value {
    let feature = features(body).iter().find(|feature| feature["id"].as_str() == Some(id));
    assert!(feature.is_some(), "no feature with id {id}: {body}");
    feature.expect("the feature was just asserted to be present")
}

/// Clause 6.4.3.2: a `join=flat` response is a collection, so its GeoJSON
/// rendering is a FeatureCollection — on the query binding and on the
/// single-entity retrieval alike, which the flat join also turns into a
/// collection. Clause 6.3.8 `geometryProperty` selects the geometry on both.
#[tokio::test]
async fn join_flat_geojson_returns_feature_collection() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_b = scope.id("Building:geo-B");
    let id_a = scope.id("Building:geo-A");
    create(client, &located_building(&id_b, 7.1)).await;
    create(client, &located_linked_building(&id_a, 13.3986, &id_b)).await;

    let queried = query_geojson(client, &format!("id={id_a}&type=Building&join=flat&joinLevel=1")).await;
    assert_eq!(queried["type"], "FeatureCollection");
    let ids = feature_ids(&queried);
    assert!(ids.contains(&id_a), "the matching entity is a feature: {ids:?}");
    assert!(ids.contains(&id_b), "the linked entity is a feature: {ids:?}");

    let retrieved = get_geojson(client, &id_a, "join=flat&joinLevel=1").await;
    assert_eq!(
        retrieved["type"], "FeatureCollection",
        "a flat join on a single GET is a collection: {retrieved}"
    );
    let ids = feature_ids(&retrieved);
    assert!(ids.contains(&id_a), "the retrieved entity is a feature: {ids:?}");
    assert!(ids.contains(&id_b), "the linked entity is a feature: {ids:?}");

    let selected = get_geojson(client, &id_a, "join=flat&joinLevel=1&geometryProperty=observationPoint").await;
    let feature = feature_for(&selected, &id_a);
    let longitude = feature["geometry"]["coordinates"][0]
        .as_f64()
        .unwrap_or_else(|| panic!("feature geometry must carry coordinates: {feature}"));
    assert!(
        (longitude - 13.3986).abs() < 0.001,
        "geometryProperty must select observationPoint on a flat GET, got lon={longitude}"
    );
    scope.finish().await.expect("cleanup failed");
}
