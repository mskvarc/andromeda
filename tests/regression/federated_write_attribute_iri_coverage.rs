//! A registration covers a written attribute by its expanded IRI, never by the
//! last segment of that IRI.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.12 matches a registration's `propertyNames`
//! and `relationshipNames` against the expanded names of the operation. Two
//! different IRIs that happen to end in the same word (`http://a.example/v#speed`
//! and `http://b.example/ns/speed`) name different attributes, so a write of the
//! first is not forwarded to a registration declaring only the second.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::context_source_adapter::ContextSourceAdapter;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

/// The IRI the written `speed` term expands to.
const WRITTEN_SPEED: &str = "http://a.example/v#speed";

/// An `inclusive` registration of this test's `covered_type` entities declaring
/// the single property name `declared_name` (an absolute IRI, so it expands to
/// itself under any context), advertising `updateEntity`.
fn speed_registration(scope: &TestEnvironment, covered_type: &str, declared_name: &str, endpoint: &str) -> Value {
    json!({
        "id": scope.id("registration:attribute-iri"),
        "type": "ContextSourceRegistration",
        "endpoint": endpoint,
        "mode": "inclusive",
        "operations": ["updateEntity"],
        "information": [{
            "entities": [{"type": covered_type, "idPattern": format!("^{}:", scope.id_prefix())}],
            "propertyNames": [declared_name]
        }],
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// A local entity holding `speed` under `WRITTEN_SPEED`.
fn entity_with_written_speed(id: &str, entity_type: &str) -> Value {
    json!({
        "id": id,
        "type": entity_type,
        "speed": {"type": "Property", "value": 10},
        "@context": [TEST_SUITE_CONTEXT, {"speed": WRITTEN_SPEED}]
    })
}

/// Create the registration declaring `declared_name`, then the local entity, then
/// update its `speed`; return the source that the registration names.
async fn update_speed_under_registration(scope: &TestEnvironment, declared_name: &str) -> ContextSourceAdapter {
    let client = scope.client();
    let source = ContextSourceAdapter::start().await;
    let covered_type = scope.entity_type("AttributeIriCoverage");

    let created = client
        .create_registration(&speed_registration(scope, &covered_type, declared_name, &source.url("/")))
        .await
        .expect("create registration");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let entity_id = scope.id("entity:attribute-iri");
    let created = client
        .post_entity(&entity_with_written_speed(&entity_id, &covered_type))
        .send()
        .await
        .expect("create entity");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let updated = client
        .update_entity_attrs(
            &entity_id,
            &json!({
                "speed": {"type": "Property", "value": 20},
                "@context": [TEST_SUITE_CONTEXT, {"speed": WRITTEN_SPEED}]
            }),
        )
        .await
        .expect("update attributes");
    assert!(
        updated.status.is_success() || updated.status == StatusCode::MULTI_STATUS,
        "update over an inclusive registration: {:?}",
        updated.body
    );
    source
}

#[tokio::test]
async fn regression_federated_attribute_iri_a_different_iri_with_the_same_last_segment_is_not_forwarded() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let source = update_speed_under_registration(&scope, "http://b.example/ns/speed").await;

    assert_eq!(
        source.get_total_count(),
        0,
        "http://a.example/v#speed is not the registered http://b.example/ns/speed, so nothing is forwarded"
    );

    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn regression_federated_attribute_iri_the_identical_iri_is_forwarded() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let source = update_speed_under_registration(&scope, WRITTEN_SPEED).await;

    let forwarded = source.next_request(Duration::from_secs(5)).await;
    assert_eq!(forwarded.method, Method::PATCH, "the registered IRI is covered, so the update is forwarded");

    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}
