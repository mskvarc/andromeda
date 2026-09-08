//! Simplified-GeoJSON rendering tests (NGSI-LD 4.5.17).
//!
//! `options=keyValues` combined with `Accept: application/geo+json` must
//! produce a `FeatureCollection` whose `properties` are flattened
//! key-value pairs, not the normalized Property objects.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::header::jsonld_link_header;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

#[tokio::test]
async fn spatial_simplified_geojson_flattens_properties_in_feature() {
    let scope = TestEnvironment::from_suite("spatial_simplified").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:simplified-geojson");

    let create = client
        .post_entity(&json!({
            "id": id,
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "height": {"type": "Property", "value": 330},
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [2.2945, 48.8584]}
            },
            "@context": [TEST_SUITE_CONTEXT]
        }))
        .send()
        .await
        .expect("create entity failed");
    assert_eq!(create.status, StatusCode::CREATED);

    let query = format!("/ngsi-ld/v1/entities?id={}&type=Building&options=keyValues", urlencoding::encode(&id));
    let response = client
        .raw(Method::GET, &query)
        .header("Accept", "application/geo+json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test context Link must be valid"))
        .send()
        .await
        .expect("GeoJSON query failed");
    assert_eq!(response.status, StatusCode::OK);
    let body = response.json_value().expect("response body must be valid JSON");
    assert_eq!(body["type"], "FeatureCollection");

    let features = body["features"].as_array().expect("features array missing");
    let feature = features
        .iter()
        .find(|feature| feature["id"].as_str() == Some(&id))
        .expect("feature for created entity must be present");

    let properties = feature["properties"].as_object().expect("properties must be an object");
    // keyValues must flatten: properties.name is the scalar, not an object
    // carrying `type`/`value`.
    assert_eq!(properties.get("name"), Some(&Value::String("Eiffel Tower".into())));
    assert_eq!(properties.get("height"), Some(&json!(330)));
    // Location stays at geometry, not properties.
    assert_eq!(feature["geometry"]["type"], "Point");

    scope.finish().await.expect("cleanup failed");
}
