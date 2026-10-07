//! Clause 4.5.2.3 concise representation that reads back as what was written.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.5.2.3 calls the concise representation
//! "concise but lossless". A Property holding a `GeoJSON` geometry reads back as
//! a Property, not a `GeoProperty`; one holding an object with a `type`, another
//! family's value member or `value` reads back with that object as its value,
//! not as an attribute of its own; the default instance of a multi-attribute is
//! written as an object, not a bare element of the array; and the reading takes a
//! typeless `{"value": …}` instance, in an array or alone, as an instance. Each
//! concise answer here is written back as a new entity and read normalized, so
//! the round trip is asserted, not only the shape.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The problem type clause 5.5.4 gives data that is not syntactically correct
/// according to the API data type definitions.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// One `Device` carrying `attributes`.
fn device(id: &str, attributes: &Value) -> Value {
    let mut entity = json!({"id": id, "type": "Device", "@context": [TEST_SUITE_CONTEXT]});
    if let (Some(target), Some(attributes)) = (entity.as_object_mut(), attributes.as_object()) {
        target.extend(attributes.iter().map(|(name, node)| (name.clone(), node.clone())));
    }
    entity
}

/// Create `entity` and assert it is created.
async fn create(scope: &TestEnvironment, entity: &Value) {
    let created = scope.client().post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create {entity}: {:?}", created.body);
}

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

/// Write the concise answer `concise` back as the entity `id` and read it
/// normalized: what the concise representation said, as a reader takes it.
async fn written_back(scope: &TestEnvironment, id: &str, concise: &Value) -> Value {
    let mut entity = concise.clone(); // clone: the concise answer is asserted on by the caller too
    let members = entity.as_object_mut().expect("an entity is an object");
    members.insert("id".to_string(), json!(id));
    members.insert("@context".to_string(), json!([TEST_SUITE_CONTEXT]));
    create(scope, &entity).await;
    read(scope, id, "").await
}

/// Clause 5.2.5: in the concise representation "the value member cannot be a
/// `GeoJSON` Object ... as it would be interpreted as a `GeoProperty`", so a
/// Property holding a geometry keeps its `type`.
#[tokio::test]
async fn regression_concise_lossless_a_property_holding_a_geometry_keeps_its_type() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("Device:concise-geometry-property");
    let geometry = json!({"type": "Point", "coordinates": [13.4, 52.5]});
    create(&scope, &device(&id, &json!({"pin": {"type": "Property", "value": geometry}}))).await;

    let concise = read(&scope, &id, "options=concise").await;
    assert_eq!(concise["pin"], json!({"type": "Property", "value": geometry}), "{concise}");

    let normalized = written_back(&scope, &scope.id("Device:concise-geometry-property-back"), &concise).await;
    assert_eq!(
        normalized["pin"]["type"],
        json!("Property"),
        "the geometry stays a Property value: {normalized}"
    );
    assert_eq!(normalized["pin"]["value"], geometry, "{normalized}");

    scope.finish().await.expect("cleanup failed");
}

/// The same holds for a Property carrying metadata beside the geometry, whose
/// typeless object clause 4.5.2.3 would read as a `GeoProperty`.
#[tokio::test]
async fn regression_concise_lossless_a_property_holding_a_geometry_beside_metadata_keeps_its_type() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("Device:concise-geometry-property-observed");
    let geometry = json!({"type": "Point", "coordinates": [13.4, 52.5]});
    let pin = json!({"type": "Property", "value": geometry, "observedAt": "2025-01-01T00:00:00Z"});
    create(&scope, &device(&id, &json!({"pin": pin}))).await;

    let concise = read(&scope, &id, "options=concise").await;
    assert_eq!(concise["pin"], pin, "{concise}");

    let normalized = written_back(&scope, &scope.id("Device:concise-geometry-property-observed-back"), &concise).await;
    assert_eq!(normalized["pin"]["type"], json!("Property"), "{normalized}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.2.3 reads a bare object carrying a `type`, another family's value
/// member or `value` as an attribute, and an array of such objects as
/// instances, so a Property holding one is written under `value`.
#[tokio::test]
async fn regression_concise_lossless_a_value_that_would_read_as_an_attribute_keeps_its_value_member() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("Device:concise-attribute-shaped-values");
    let values = json!({
        "feature": {"type": "Feature", "x": 1},
        "pointer": {"object": "urn:ngsi-ld:Thing:1"},
        "nested": {"value": 1, "unit": "x"},
        "pointers": [{"object": "urn:ngsi-ld:Thing:1"}, {"object": "urn:ngsi-ld:Thing:2"}]
    });
    let attributes: Value = values
        .as_object()
        .expect("an object")
        .iter()
        .map(|(name, value)| (name.clone(), json!({"type": "Property", "value": value})))
        .collect::<serde_json::Map<String, Value>>()
        .into();
    create(&scope, &device(&id, &attributes)).await;

    let concise = read(&scope, &id, "options=concise").await;
    let normalized = written_back(&scope, &scope.id("Device:concise-attribute-shaped-values-back"), &concise).await;
    for (name, value) in values.as_object().expect("an object") {
        assert_eq!(concise[name], json!({"value": value}), "{name}: {concise}");
        assert_eq!(normalized[name]["type"], json!("Property"), "{name}: {normalized}");
        assert_eq!(&normalized[name]["value"], value, "{name}: {normalized}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// A value the reading takes for a value still collapses to the bare value
/// clause 4.5.2.3 writes for a Property without sub-attributes.
#[tokio::test]
async fn regression_concise_lossless_a_plain_value_still_collapses() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("Device:concise-plain-values");
    create(
        &scope,
        &device(
            &id,
            &json!({
                "count": {"type": "Property", "value": 42},
                "settings": {"type": "Property", "value": {"a": 1}},
                "tags": {"type": "Property", "value": ["a", "b"]}
            }),
        ),
    )
    .await;

    let concise = read(&scope, &id, "options=concise").await;
    assert_eq!(concise["count"], json!(42), "{concise}");
    assert_eq!(concise["settings"], json!({"a": 1}), "{concise}");
    assert_eq!(concise["tags"], json!(["a", "b"]), "{concise}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.2.3 writes several instances as "a JSON-LD array with such
/// JSON-LD objects", so the default instance stays an object too, and the
/// array reads back as the same two instances.
#[tokio::test]
async fn regression_concise_lossless_every_instance_of_a_multi_attribute_keeps_its_object_form() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("Device:concise-multi-attribute");
    create(
        &scope,
        &device(
            &id,
            &json!({"speed": [
                {"type": "Property", "value": 25, "datasetId": "urn:ngsi-ld:Dataset:a"},
                {"type": "Property", "value": 30}
            ]}),
        ),
    )
    .await;

    let concise = read(&scope, &id, "options=concise").await;
    let instances = concise["speed"].as_array().expect("a multi-attribute is an array");
    assert_eq!(instances.len(), 2, "{concise}");
    assert!(instances.iter().all(Value::is_object), "every instance is an object: {concise}");
    assert!(instances.contains(&json!({"value": 30})), "the default instance is an object: {concise}");

    let normalized = written_back(&scope, &scope.id("Device:concise-multi-attribute-back"), &concise).await;
    let instances = normalized["speed"].as_array().expect("two instances read back");
    assert_eq!(instances.len(), 2, "{normalized}");
    assert!(instances.iter().all(|instance| instance["type"] == json!("Property")), "{normalized}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.2.3 infers a Property "by the presence of the "value" attribute",
/// so an array of typeless instances is the instances of a multi-attribute
/// (clause 4.5.5), not one array value.
#[tokio::test]
async fn regression_concise_lossless_typeless_instances_in_an_array_are_instances() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("Device:concise-typeless-instances");
    create(
        &scope,
        &device(
            &id,
            &json!({"speed": [
                {"value": 25, "datasetId": "urn:ngsi-ld:Dataset:a"},
                {"value": 30, "datasetId": "urn:ngsi-ld:Dataset:b"}
            ]}),
        ),
    )
    .await;

    let normalized = read(&scope, &id, "").await;
    let instances = normalized["speed"].as_array().expect("two instances");
    assert_eq!(instances.len(), 2, "{normalized}");
    for (dataset, value) in [("urn:ngsi-ld:Dataset:a", 25), ("urn:ngsi-ld:Dataset:b", 30)] {
        assert!(
            instances.contains(&json!({"type": "Property", "value": value, "datasetId": dataset})),
            "{dataset}: {normalized}"
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// An array holding instances and values alike is neither the instances of a
/// multi-attribute nor one value, and is refused.
#[tokio::test]
async fn regression_concise_lossless_an_array_mixing_instances_and_values_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("Device:concise-mixed-array");
    let entity = device(&id, &json!({"speed": [{"value": 25, "datasetId": "urn:ngsi-ld:Dataset:a"}, 30]}));

    let response = scope.client().post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{problem}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.2.3 infers a Property from `value` whether or not a sub-attribute
/// is written beside it, and a `GeoProperty` when the value is a geometry.
#[tokio::test]
async fn regression_concise_lossless_a_lone_value_member_is_the_property_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("Device:concise-lone-value");
    let geometry = json!({"type": "Point", "coordinates": [1.0, 2.0]});
    create(&scope, &device(&id, &json!({"count": {"value": 42}, "spot": {"value": geometry}}))).await;

    let normalized = read(&scope, &id, "").await;
    assert_eq!(normalized["count"], json!({"type": "Property", "value": 42}), "{normalized}");
    assert_eq!(normalized["spot"], json!({"type": "GeoProperty", "value": geometry}), "{normalized}");

    scope.finish().await.expect("cleanup failed");
}
