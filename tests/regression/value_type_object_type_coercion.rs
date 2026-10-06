//! `valueType` and `objectType` are any string the `@context` coerces to a URI.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.5.2.2 types `valueType` as "a string value
//! which shall be type coerced into a datatype URI", and Tables 5.2.5-1 and
//! 5.2.36-1 as "a String Value which shall be type coerced to a URI based on
//! the supplied @context". Tables 5.2.6-1 and 5.2.37-1 allow `objectType` "short
//! hand string(s) (type name) or URI(s)". None restricts the string to the
//! clause 4.6.2 short-name grammar, which has no `-`, so a hyphenated term the
//! user `@context` defines is a valid value. Reading the compact body typed
//! before expansion held both members to that grammar, so the entity was
//! refused with `400 BadRequestData`.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// The IRI the user `@context` gives the datatype term `my-datatype`.
const MY_DATATYPE: &str = "https://example.org/datatype/my-datatype";

/// The IRI the user `@context` gives the type term `smart-building`.
const SMART_BUILDING: &str = "https://example.org/type/smart-building";

/// The IRI the user `@context` gives the type term `smart-room`.
const SMART_ROOM: &str = "https://example.org/type/smart-room";

/// Clauses 4.5.2.2 and 5.2.6: a Property with a hyphenated `valueType` and
/// Relationships with a hyphenated `objectType`, single and array, are created
/// and read back with each term expanded to the IRI the user `@context` defines.
#[tokio::test]
async fn regression_value_type_object_type_hyphenated_terms_are_created_and_read_back() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:coerced-types");
    let target = scope.id("Building:coerced-types");
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "reading": {"type": "Property", "value": "42", "valueType": "my-datatype"},
        "garage": {"type": "Relationship", "object": target, "objectType": "smart-building"},
        "garages": {"type": "Relationship", "object": target, "objectType": ["smart-building", "smart-room"]},
        "@context": [CORE_CONTEXT, {
            "my-datatype": MY_DATATYPE,
            "smart-building": SMART_BUILDING,
            "smart-room": SMART_ROOM
        }]
    });
    let created = client.post_entity(&entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let retrieved = client.get_entity(&id, "", "application/ld+json", None).await.expect("retrieve entity request");
    scope.finish().await.expect("environment cleanup");

    assert_eq!(retrieved.status, StatusCode::OK, "{:?}", retrieved.body);
    let body = retrieved.json_value().expect("an entity is JSON");
    assert_eq!(body["reading"]["valueType"], json!(MY_DATATYPE), "{body}");
    assert_eq!(body["garage"]["objectType"], json!(SMART_BUILDING), "{body}");
    assert_eq!(body["garages"]["objectType"], json!([SMART_BUILDING, SMART_ROOM]), "{body}");
}
