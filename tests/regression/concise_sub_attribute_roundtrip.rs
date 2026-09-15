//! Clause 4.5.2.3 concise input carrying a sub-attribute beside its value.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.5.2.3 lets a client write an attribute
//! concisely: `{"value": 42, "accuracy": 0.5}` is a Property whose value is `42`
//! and whose `accuracy` is a sub-attribute, and a reserved member such as
//! `unitCode` stays in the envelope. The normalizer used to wrap the whole
//! typeless object as one opaque Property value, burying the sub-attribute (and
//! `unitCode`) inside `value`. This suite proves the concise shape survives a
//! create-then-read round trip, in both the normalized and the concise
//! representation.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// Read one entity back in the representation `query` selects.
async fn read(scope: &TestEnvironment, id: &str, query: &str) -> Value {
    let response = scope
        .client()
        .get_entity(id, query, "application/json", Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("retrieve entity failed");
    assert_eq!(response.status, StatusCode::OK, "retrieve {query}: {:?}", response.body);
    response.json_value().expect("retrieve response must be JSON")
}

/// A concise attribute carrying a sub-attribute (no explicit `type`) survives a
/// round trip: the value stays scalar and the sub-attribute is a first-class
/// sub-attribute in both representations.
#[tokio::test]
async fn regression_concise_sub_attribute_roundtrip_a_sibling_survives_as_a_sub_attribute() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("Device:concise-sub-attribute");

    let entity = json!({
        "id": id,
        "type": "Device",
        // Concise: no `type`, a scalar `value`, a user sub-attribute, and a
        // reserved `unitCode` that must stay in the envelope.
        "temperature": { "value": 42, "accuracy": 0.5, "unitCode": "CEL" },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = scope.client().post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // Normalized (default): the value is the scalar, `accuracy` is its own
    // Property sub-attribute, and `unitCode` sits in the envelope.
    let normalized = read(&scope, &id, "").await;
    let temperature = &normalized["temperature"];
    assert_eq!(temperature["type"], json!("Property"), "the attribute is a Property: {temperature}");
    assert_eq!(
        temperature["value"],
        json!(42),
        "the value stays the scalar, not the whole object: {temperature}"
    );
    assert_eq!(temperature["unitCode"], json!("CEL"), "unitCode stays in the envelope: {temperature}");
    assert_eq!(
        temperature["accuracy"]["type"],
        json!("Property"),
        "the sub-attribute is a first-class Property: {temperature}"
    );
    assert_eq!(temperature["accuracy"]["value"], json!(0.5), "the sub-attribute keeps its value: {temperature}");
    assert!(
        temperature["value"].get("accuracy").is_none(),
        "the sub-attribute is not buried inside the value: {temperature}"
    );

    // Concise: the round trip returns the same abbreviated shape.
    let concise = read(&scope, &id, "options=concise").await;
    let temperature = &concise["temperature"];
    assert_eq!(temperature["value"], json!(42), "concise value is the scalar: {temperature}");
    assert_eq!(temperature["accuracy"], json!(0.5), "concise sub-attribute is its bare value: {temperature}");
    assert_eq!(temperature["unitCode"], json!("CEL"), "concise keeps unitCode: {temperature}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.2.3 geospatial exception: a concise attribute whose `value`
/// resolves to a `GeoJSON` geometry, carried beside a sibling, is stored as a
/// `GeoProperty` rather than a `Property` -- a moving entity's location with an
/// `observedAt` is the common case.
#[tokio::test]
async fn regression_concise_sub_attribute_roundtrip_a_geometry_value_infers_a_geo_property() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("Device:concise-geo");

    let entity = json!({
        "id": id,
        "type": "Device",
        // Concise, typeless: a GeoJSON geometry value beside an observedAt sibling.
        "location": {"value": {"type": "Point", "coordinates": [13.4, 52.5]}, "observedAt": "2024-01-01T00:00:00Z"},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = scope.client().post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let normalized = read(&scope, &id, "").await;
    let location = &normalized["location"];
    assert_eq!(location["type"], json!("GeoProperty"), "the geometry value infers a GeoProperty: {location}");
    assert_eq!(location["value"]["type"], json!("Point"), "the geometry is the value: {location}");
    assert_eq!(
        location["observedAt"],
        json!("2024-01-01T00:00:00Z"),
        "observedAt stays in the envelope: {location}"
    );

    scope.finish().await.expect("cleanup failed");
}
