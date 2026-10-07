//! ETSI GS CIM 009 v1.9.1 clause 4.5.2.3 treats an object carrying a `type` as a normalized attribute, clause 5.2.4 confines that `type` to eight tokens, and clause 4.5.2.2 makes a sub-attribute an attribute, so the eight-token rule holds at every depth. Each violation is refused as `BadRequestData` (clause 5.5.4).

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::response::TestResponse;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The problem type clause 5.5.4 gives data that is not syntactically correct
/// according to the API data type definitions.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// Assert a refusal is the `BadRequestData` of clause 5.5.4, naming `at`.
fn assert_bad_request_data(response: &TestResponse, at: &str, scenario: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{scenario}: {problem}");
    let detail = problem["detail"].as_str().unwrap_or_default();
    assert!(detail.contains(at), "{scenario}: the detail names {at}: {problem}");
}

/// One `Device` carrying the single attribute under test.
fn device_with(id: &str, attribute: &str, node: &Value) -> Value {
    json!({"id": id, "type": "Device", attribute: node, "@context": [TEST_SUITE_CONTEXT]})
}

/// Create `entity` and assert the broker refuses it as `BadRequestData` naming
/// `at`.
async fn assert_create_refused(scope: &TestEnvironment, entity: &Value, at: &str, scenario: &str) {
    let response = scope.client().post_entity(entity).send().await.expect("create entity request");
    assert_bad_request_data(&response, at, scenario);
}

/// Clause 5.2.4 holds for a sub-attribute's `type` too.
#[tokio::test]
async fn regression_attribute_node_a_misspelt_sub_attribute_type_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity = device_with(
        &scope.id("Device:misspelt-sub-attribute"),
        "speed",
        &json!({"type": "Property", "value": 1, "accuracy": {"type": "Propertyy", "value": 0.5}}),
    );
    assert_create_refused(&scope, &entity, "/speed/accuracy", "a misspelt sub-attribute type").await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.2.3: an object carrying a `type` is a normalized attribute, so a sub-attribute naming no attribute type is refused rather than kept as a value.
#[tokio::test]
async fn regression_attribute_node_a_sub_attribute_naming_no_attribute_type_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity = device_with(
        &scope.id("Device:feature-sub-attribute"),
        "speed",
        &json!({"type": "Property", "value": 1, "accuracy": {"type": "Feature", "x": 1}}),
    );
    assert_create_refused(&scope, &entity, "/speed/accuracy", "a sub-attribute typed Feature").await;
    scope.finish().await.expect("cleanup failed");
}

/// The same rule holds on Append Entity Attributes (clause 5.6.3).
#[tokio::test]
async fn regression_attribute_node_a_misspelt_sub_attribute_type_is_refused_on_append() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("Device:misspelt-sub-attribute-append");
    let created = scope
        .client()
        .post_entity(&device_with(&id, "speed", &json!({"type": "Property", "value": 1})))
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let fragment = json!({
        "speed": {"type": "Property", "value": 2, "accuracy": {"type": "Propertyy", "value": 0.5}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = scope.client().append_entity_attrs(&id, &fragment).await.expect("append request");
    assert_bad_request_data(&response, "/speed/accuracy", "a misspelt sub-attribute type on append");

    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.5-1 makes `type` a String; an object whose `type` is anything else is no attribute and no value.
#[tokio::test]
async fn regression_attribute_node_a_type_that_is_not_a_string_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let top = device_with(&scope.id("Device:numeric-type"), "raw", &json!({"type": 7, "value": 1}));
    assert_create_refused(&scope, &top, "/raw", "a numeric type").await;
    let nested = device_with(
        &scope.id("Device:numeric-sub-attribute-type"),
        "speed",
        &json!({"type": "Property", "value": 1, "accuracy": {"type": 7, "value": 0.5}}),
    );
    assert_create_refused(&scope, &nested, "/speed/accuracy", "a numeric sub-attribute type").await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.7.1 and IETF RFC 7946 section 3.1: a geometry is its type together with its coordinates.
#[tokio::test]
async fn regression_attribute_node_a_geometry_type_without_coordinates_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity = device_with(&scope.id("Device:geometry-without-coordinates"), "spot", &json!({"type": "Point"}));
    assert_create_refused(&scope, &entity, "/spot", "a geometry type without coordinates").await;
    scope.finish().await.expect("cleanup failed");
}

/// Each value member names one family (clauses 4.5.3.3 and 4.5.18.3), and an attribute has one.
#[tokio::test]
async fn regression_attribute_node_two_value_members_on_one_attribute_are_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity = device_with(
        &scope.id("Device:two-value-members"),
        "both",
        &json!({"object": "urn:ngsi-ld:Thing:1", "languageMap": {"en": "x"}}),
    );
    assert_create_refused(&scope, &entity, "/both", "two value members").await;
    scope.finish().await.expect("cleanup failed");
}

/// A Property's `value` is data: a `type` member inside it declares nothing.
#[tokio::test]
async fn regression_attribute_node_a_type_member_inside_a_property_value_is_data() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("Device:typed-value");
    let created = scope
        .client()
        .post_entity(&device_with(&id, "shape", &json!({"type": "Property", "value": {"type": "Feature", "x": 1}})))
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let response = scope
        .client()
        .get_entity(&id, "", "application/json", Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("retrieve entity request");
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let entity = response.json_value().expect("retrieve response must be JSON");
    assert_eq!(entity["shape"], json!({"type": "Property", "value": {"type": "Feature", "x": 1}}), "{entity}");

    scope.finish().await.expect("cleanup failed");
}
