//! Altitude across the alternative representations (NGSI-LD 4.5.16, 6.3.15,
//! 4.5.4, 4.5.19).
//!
//! GeoJSON, keyValues and concise representations all reshape an entity, and
//! none of them may drop an ordinate while doing so.

use crate::fixtures::{assert_coordinates_eq, building_point_2d, building_point_3d, create, id_filter, location_coordinates};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// Retrieves an entity as a GeoJSON Feature.
async fn get_geojson(client: &TestClient, id: &str, query: &str) -> Value {
    let response = client
        .get_entity(id, query, "application/geo+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("GeoJSON retrieve failed");
    assert_eq!(response.status, StatusCode::OK);
    response.json_value().expect("GeoJSON body must be valid JSON")
}

/// Retrieves an entity with an arbitrary query string and Accept header.
async fn get_with(client: &TestClient, id: &str, query: &str) -> Value {
    let response = client
        .get_entity(id, query, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve failed");
    assert_eq!(response.status, StatusCode::OK);
    response.json_value().expect("body must be valid JSON")
}

#[tokio::test]
async fn representation_geojson_single_entity_3d_geometry() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:geojson3d");

    create(client, &building_point_3d(&id, 13.4, 52.52, 300.0)).await;

    let feature = get_geojson(client, &id, "").await;
    assert_eq!(feature["type"], json!("Feature"));
    assert_coordinates_eq(
        feature.pointer("/geometry/coordinates").expect("Feature must carry a geometry"),
        &json!([13.4, 52.52, 300.0]),
    );
    // NGSI-LD 6.3.15: the GeoProperty also survives inside properties.
    assert_coordinates_eq(
        feature
            .pointer("/properties/location/value/coordinates")
            .expect("Feature properties must keep location"),
        &json!([13.4, 52.52, 300.0]),
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn representation_geojson_collection_mixed_2d_3d() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let elevated = scope.id("Building:gj_collection_3d");
    let flat = scope.id("Building:gj_collection_2d");

    create(client, &building_point_3d(&elevated, 13.4, 52.52, 300.0)).await;
    create(client, &building_point_2d(&flat, 13.4, 52.52)).await;

    let filter = id_filter(&[&elevated, &flat]);
    let response = client
        .query_entities(&format!("type=Building&id={filter}"), "application/geo+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("GeoJSON query failed");
    assert_eq!(response.status, StatusCode::OK);

    let collection = response.json_value().expect("FeatureCollection must be valid JSON");
    assert_eq!(collection["type"], json!("FeatureCollection"));
    let features = collection["features"].as_array().map(Vec::as_slice).unwrap_or_default();

    for feature in features {
        let id = feature.get("id").and_then(Value::as_str).expect("each Feature carries an id");
        let coordinates = feature
            .pointer("/geometry/coordinates")
            .and_then(Value::as_array)
            .expect("each Feature carries geometry coordinates");
        if id == elevated {
            assert_eq!(coordinates.len(), 3, "the 3D Feature must keep its altitude");
        } else if id == flat {
            assert_eq!(coordinates.len(), 2, "the 2D Feature must not gain an altitude");
        }
    }

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn representation_geojson_custom_geometry_property_3d() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:gj_custom");

    let entity = json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0.0, 0.0]}},
        "observationPoint": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52, 640.0]}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    create(client, &entity).await;

    // NGSI-LD 6.3.15: geometryProperty selects which GeoProperty becomes the
    // Feature geometry.
    let feature = get_geojson(client, &id, "geometryProperty=observationPoint").await;
    assert_coordinates_eq(
        feature.pointer("/geometry/coordinates").expect("Feature must carry a geometry"),
        &json!([13.4, 52.52, 640.0]),
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn representation_keyvalues_3d_location() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:keyvalues3d");

    create(client, &building_point_3d(&id, 13.4, 52.52, 300.0)).await;

    // NGSI-LD 4.5.4: format=keyValues and the legacy options=keyValues alias
    // must produce the same simplified representation.
    for query in ["format=keyValues", "options=keyValues"] {
        let body = get_with(client, &id, query).await;
        assert_coordinates_eq(
            body.pointer("/location/coordinates").expect("keyValues location must be the bare geometry"),
            &json!([13.4, 52.52, 300.0]),
        );
    }

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn representation_concise_3d_location() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:concise3d");

    create(client, &building_point_3d(&id, 13.4, 52.52, 300.0)).await;

    // NGSI-LD 4.5.19: the concise representation drops the type discriminator
    // but keeps the value verbatim.
    let body = get_with(client, &id, "format=concise").await;
    let coordinates = body
        .pointer("/location/value/coordinates")
        .or_else(|| body.pointer("/location/coordinates"))
        .expect("concise location must expose its coordinates");
    assert_coordinates_eq(coordinates, &json!([13.4, 52.52, 300.0]));

    scope.finish().await.expect("cleanup failed");
}

/// Guards that the normalised representation, which every other suite relies
/// on, is itself altitude-preserving.
#[tokio::test]
async fn representation_normalized_3d_location() {
    let scope = TestEnvironment::from_suite("spatial_3d").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:normalized3d");

    create(client, &building_point_3d(&id, 13.4, 52.52, 300.0)).await;

    let body = get_with(client, &id, "").await;
    assert_coordinates_eq(location_coordinates(&body), &json!([13.4, 52.52, 300.0]));

    scope.finish().await.expect("cleanup failed");
}
