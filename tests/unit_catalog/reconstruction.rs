//! Unit reconstruction: a stored unit renders back exactly as it was written.
//!
//! What the write path takes out of the metadata document the read path has to
//! put back exactly: the same code, in the same JSON-LD shape, on the same
//! instance. An instance that never declared a unit must not acquire one, and a
//! client asking for a pre-1.3 representation still gets the member removed
//! under clause 4.3.6.8.

use crate::fixtures::{SUITE, default_context_iri, device_with_unit, history_sub_attribute_counts, history_unit_identities};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::Value;

#[tokio::test]
async fn a_normalized_property_and_its_sub_attribute_render_the_units_they_were_written_with() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:normalized-unit");

    let created = client
        .post_entity(&device_with_unit(&id, "KGM", "GRM"))
        .send()
        .await
        .expect("create entity request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let retrieved = client
        .get_entity(&id, "", "application/json", None)
        .await
        .expect("retrieve entity request failed");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve: {:?}", retrieved.body);
    let body = retrieved.json_value().expect("retrieved entity must be valid JSON");

    assert_eq!(body["weight"]["unitCode"], "KGM", "the instance's own unit must come back exactly: {body}");
    assert_eq!(
        body["weight"]["accuracy"]["unitCode"], "GRM",
        "a sub-attribute keeps its own unit (clause 5.2.5): {body}"
    );
    assert_eq!(
        body["serial"].get("unitCode"),
        None,
        "an attribute that declared no unit must not acquire one: {body}"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn a_concise_property_renders_the_unit_it_was_written_with() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:concise-unit");

    let created = client
        .post_entity(&device_with_unit(&id, "CEL", "CEL"))
        .send()
        .await
        .expect("create entity request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let retrieved = client
        .get_entity(&id, "options=concise", "application/json", None)
        .await
        .expect("retrieve entity request failed");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve: {:?}", retrieved.body);
    let body = retrieved.json_value().expect("retrieved entity must be valid JSON");

    // The concise representation drops `type` but keeps every member that
    // carries information, the unit among them (clause 4.5.4).
    assert_eq!(body["weight"]["unitCode"], "CEL", "the concise form must keep the unit: {body}");
    assert_eq!(body["weight"]["value"], 5, "the concise form must keep the value: {body}");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn history_holds_the_unit_in_its_column_and_not_in_the_metadata_document() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:history-unit");

    let created = client
        .post_entity(&device_with_unit(&id, "MTR", "MTR"))
        .send()
        .await
        .expect("create entity request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let weight = default_context_iri("weight");
    let identities = history_unit_identities(scope.database_pool(), &id, &weight).await;
    assert!(!identities.is_empty(), "the write must have recorded history");
    for identity in &identities {
        assert!(identity.is_some(), "a unit-bearing instance must store its identity in the column");
    }

    for elements in history_sub_attribute_counts(scope.database_pool(), &id, &weight).await {
        assert!(
            elements.is_some_and(|count| count > 0),
            "the sub-attribute is stored as an element beside the unit's column: {elements:?}"
        );
    }

    // An attribute written without a unit stores none rather than a sentinel.
    let serial = default_context_iri("serial");
    for identity in history_unit_identities(scope.database_pool(), &id, &serial).await {
        assert_eq!(identity, None, "an attribute with no unit must store no identity");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.3.6.8: a client asking for a version that predates the member gets
/// a payload with the member removed and a 203, because the broker is no longer
/// speaking authoritatively about it.
#[tokio::test]
async fn a_pre_1_3_representation_removes_the_unit_member() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:pre13-unit");

    let created = client
        .post_entity(&device_with_unit(&id, "KGM", "KGM"))
        .send()
        .await
        .expect("create entity request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let adapted = client
        .raw(Method::GET, &format!("/ngsi-ld/v1/entities/{}", urlencoding::encode(&id)))
        .header("Accept", "application/json")
        .header("Prefer", "ngsi-ld=1.0")
        .send()
        .await
        .expect("version-adapted retrieve request failed");
    assert_eq!(
        adapted.status,
        StatusCode::NON_AUTHORITATIVE_INFORMATION,
        "an adapted payload is not authoritative: {:?}",
        adapted.body
    );
    let body: Value = adapted.json_value().expect("adapted entity must be valid JSON");
    assert_eq!(
        body["weight"].get("unitCode"),
        None,
        "a pre-1.3 representation carries no unitCode member: {body}"
    );

    scope.finish().await.expect("cleanup failed");
}
