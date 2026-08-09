//! Simplified-GeoJSON rendering tests (NGSI-LD 4.5.17).
//!
//! `options=keyValues` combined with `Accept: application/geo+json` must
//! produce a FeatureCollection whose `properties` are flattened
//! key-value pairs, not the normalized Property objects.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_common::{client::NgsiLdClient, config::TEST_SUITE_CONTEXT, header::jsonld_link_header, scope::TestScope};
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

#[tokio::test]
async fn spatial_simplified_geojson_flattens_properties_in_feature() {
    let scope = TestScope::new("spatial_simplified");
    let client = NgsiLdClient::new(&scope).await;
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
        .header("Link", &jsonld_link_header(TEST_SUITE_CONTEXT))
        .send()
        .await
        .expect("GeoJSON query failed");
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body["type"], "FeatureCollection");

    let features = response.body["features"].as_array().expect("features array missing");
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

    scope.cleanup().await.expect("cleanup failed");
}
