//! A registration must be matched to an attribute-level write by the entity's
//! own types, not treated as covering every entity.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.12 matches a registration to an operation only
//! when its type selector matches the entity's types. An attribute-level operation
//! -- append, update or delete of attributes, a merge, a replace, or a delete of
//! the entity -- carries no `type` in its request, so the broker reads the local
//! entity's own types to decide coverage. It once treated the absent type as
//! satisfying every registration's type selector, so a `redirect` registration
//! scoped to an unrelated type was taken to cover the write and, advertising only
//! a read operation, refused it with a 409 Conflict. Under concurrency this
//! surfaced as an intermittent failure of any test whose write happened to run
//! while another test's type-scoped registration was live.
//!
//! These tests pin the fix: a write to a locally-held entity is unaffected by a
//! registration covering none of its types, and an entity carrying several types
//! is matched by a registration covering any one of them (clause 5.12, "the type
//! selector matches the entity types"). Both the entity types and the
//! registrations' covered types are per-test unique, so the suite is safe to run
//! in parallel.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::context_source_adapter::ContextSourceAdapter;
use andromeda_test_client::entity_attribute::DatasetSelection;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

/// A `redirect` registration covering this test's own `covered_type` entities and
/// advertising only a read operation, so any write it is taken to cover is refused
/// as not advertised. The `idPattern` scopes it to this test's id prefix, so it
/// never overlaps a concurrent test's registrations or entities. Its endpoint is
/// never contacted: a write it does not cover runs locally, and one it does cover
/// is refused before any forward is attempted.
fn redirect_registration(scope: &TestEnvironment, local_id: &str, covered_type: &str) -> Value {
    json!({
        "id": scope.id(local_id),
        "type": "ContextSourceRegistration",
        "endpoint": "http://127.0.0.1:9/unused",
        "mode": "redirect",
        "operations": ["retrieveEntity"],
        "information": [{"entities": [{"type": covered_type, "idPattern": format!("^{}:", scope.id_prefix())}]}],
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// An entity of `entity_type` carrying one property, ready for an attribute write.
fn entity(id: &str, entity_type: &str) -> Value {
    json!({
        "id": id,
        "type": entity_type,
        "speed": {"type": "Property", "value": 10},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// A fragment appending one property, with the suite context.
fn fuel_fragment() -> Value {
    json!({"fuel": {"type": "Property", "value": 5}, "@context": [TEST_SUITE_CONTEXT]})
}

/// An `inclusive` registration covering this test's own `covered_type` entities
/// and advertising the update-attributes operation, delivering to `endpoint`.
/// Inclusive, so the broker still holds the entity locally -- which lets a
/// matching update run locally *and* forward, so the mock's receipt of the
/// forward is the proof that the type matched (clause 5.12).
///
/// The advertised token is `updateEntity`: clause 4.20 Table 4.20-1 names the
/// PATCH `/entities/{id}/attrs` operation (clause 5.6.2, "update an entity's
/// attribute collection") `updateEntity`. `updateAttrs` is the distinct clause
/// 5.6.4 partial single-attribute update, which this test does not exercise.
fn inclusive_update_registration(scope: &TestEnvironment, local_id: &str, covered_type: &str, endpoint: &str) -> Value {
    json!({
        "id": scope.id(local_id),
        "type": "ContextSourceRegistration",
        "endpoint": endpoint,
        "mode": "inclusive",
        "operations": ["updateEntity"],
        "information": [{"entities": [{"type": covered_type, "idPattern": format!("^{}:", scope.id_prefix())}]}],
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// A local entity carrying two types, ready for an attribute write that names none.
fn dual_typed_entity(id: &str, first: &str, second: &str) -> Value {
    json!({
        "id": id,
        "type": [first, second],
        "speed": {"type": "Property", "value": 10},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Create a `redirect` registration covering one per-test type, then a local
/// entity of a *different* per-test type, and return the entity id and its type.
async fn registration_over_an_unrelated_type(scope: &TestEnvironment, tag: &str) -> (String, String) {
    let client = scope.client();
    let covered = scope.entity_type("CoverageRegisteredType");
    let registered = client
        .create_registration(&redirect_registration(scope, &format!("registration:{tag}"), &covered))
        .await
        .expect("create registration");
    assert_eq!(registered.status, StatusCode::CREATED, "registration: {:?}", registered.body);

    let local_type = scope.entity_type("CoverageLocalType");
    let entity_id = scope.id(&format!("entity:{tag}"));
    let created = client.post_entity(&entity(&entity_id, &local_type)).send().await.expect("create entity");
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "an entity of a type the registration does not cover is created locally: {:?}",
        created.body
    );
    (entity_id, local_type)
}

#[tokio::test]
async fn regression_federated_coverage_append_attrs_to_an_unrelated_type_is_not_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let (entity_id, _type) = registration_over_an_unrelated_type(&scope, "append").await;
    let client = scope.client();

    let response = client.append_entity_attrs(&entity_id, &fuel_fragment()).await.expect("append attributes");
    assert_eq!(
        response.status,
        StatusCode::NO_CONTENT,
        "appending to a locally-held entity of an unrelated type must not be refused: {:?}",
        response.body
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn regression_federated_coverage_update_attrs_of_an_unrelated_type_is_not_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let (entity_id, _type) = registration_over_an_unrelated_type(&scope, "update").await;
    let client = scope.client();

    let response = client
        .update_entity_attrs(
            &entity_id,
            &json!({"speed": {"type": "Property", "value": 20}, "@context": [TEST_SUITE_CONTEXT]}),
        )
        .await
        .expect("update attributes");
    assert_eq!(
        response.status,
        StatusCode::NO_CONTENT,
        "updating attributes of an unrelated type must not be refused: {:?}",
        response.body
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn regression_federated_coverage_partial_update_of_an_unrelated_type_is_not_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let (entity_id, _type) = registration_over_an_unrelated_type(&scope, "partial").await;
    let client = scope.client();

    let response = client
        .partial_update_attr(&entity_id, "speed", &json!({"type": "Property", "value": 30, "@context": [TEST_SUITE_CONTEXT]}))
        .await
        .expect("partial update attribute");
    assert_eq!(
        response.status,
        StatusCode::NO_CONTENT,
        "a partial attribute update of an unrelated type must not be refused: {:?}",
        response.body
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn regression_federated_coverage_delete_attr_of_an_unrelated_type_is_not_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let (entity_id, _type) = registration_over_an_unrelated_type(&scope, "delete-attr").await;
    let client = scope.client();

    let response = client
        .delete_entity_attr(&entity_id, "speed", None, DatasetSelection::Default, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("delete attribute");
    assert_eq!(
        response.status,
        StatusCode::NO_CONTENT,
        "deleting an attribute of an unrelated type must not be refused: {:?}",
        response.body
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn regression_federated_coverage_merge_of_an_unrelated_type_is_not_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let (entity_id, _type) = registration_over_an_unrelated_type(&scope, "merge").await;
    let client = scope.client();

    let response = client
        .merge_entity(&entity_id, &fuel_fragment(), Some(TEST_SUITE_CONTEXT))
        .await
        .expect("merge entity");
    assert_eq!(
        response.status,
        StatusCode::NO_CONTENT,
        "merging a locally-held entity of an unrelated type must not be refused: {:?}",
        response.body
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn regression_federated_coverage_replace_of_an_unrelated_type_is_not_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let (entity_id, local_type) = registration_over_an_unrelated_type(&scope, "replace").await;
    let client = scope.client();

    let mut replacement = entity(&entity_id, &local_type);
    replacement["speed"] = json!({"type": "Property", "value": 40});
    let response = client
        .replace_entity(&entity_id, &replacement, "application/ld+json", None)
        .await
        .expect("replace entity");
    assert_eq!(
        response.status,
        StatusCode::NO_CONTENT,
        "replacing a locally-held entity of an unrelated type must not be refused: {:?}",
        response.body
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn regression_federated_coverage_delete_entity_of_an_unrelated_type_is_not_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let (entity_id, _type) = registration_over_an_unrelated_type(&scope, "delete-entity").await;
    let client = scope.client();

    let response = client.delete_entity(&entity_id).await.expect("delete entity");
    assert_eq!(
        response.status,
        StatusCode::NO_CONTENT,
        "deleting a locally-held entity of an unrelated type must not be refused: {:?}",
        response.body
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.12: the type selector matches the entity's *types*. An attribute
/// write names no type, so the broker resolves the local entity's types -- all of
/// them -- and a registration covering the SECOND of two types must still match,
/// forwarding the update to the source.
#[tokio::test]
async fn regression_federated_coverage_a_registration_covering_a_second_entity_type_is_matched() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let source = ContextSourceAdapter::start().await;
    let first = scope.entity_type("MultiTypeMatchFirst");
    let second = scope.entity_type("MultiTypeMatchSecond");

    let created = client
        .create_registration(&inclusive_update_registration(
            &scope,
            "registration:multitype-match",
            &second,
            &source.url("/"),
        ))
        .await
        .expect("create registration");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let entity_id = scope.id("entity:multitype-match");
    let created = client
        .post_entity(&dual_typed_entity(&entity_id, &first, &second))
        .send()
        .await
        .expect("create entity");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let updated = client
        .update_entity_attrs(
            &entity_id,
            &json!({"speed": {"type": "Property", "value": 20}, "@context": [TEST_SUITE_CONTEXT]}),
        )
        .await
        .expect("update attributes");
    assert!(
        updated.status.is_success() || updated.status == StatusCode::MULTI_STATUS,
        "update over an inclusive registration: {:?}",
        updated.body
    );

    // The forward runs inside the update's own request handling, so it has
    // already arrived by the time the client sees the response. Capturing it
    // proves the second type matched (clause 5.12): the update, a PATCH on the
    // entity's attributes, reached the source the registration named.
    let forwarded = source.next_request(Duration::from_secs(5)).await;
    assert_eq!(
        forwarded.method,
        Method::PATCH,
        "a registration covering the entity's second type must be matched, so the update is forwarded to it"
    );
    assert!(
        forwarded.uri.path().ends_with("/attrs"),
        "the forwarded update addresses the entity's attribute collection: {}",
        forwarded.uri.path()
    );

    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// The other side of clause 5.12: a registration covering none of a multi-typed
/// entity's types is not matched, so the update stays purely local.
#[tokio::test]
async fn regression_federated_coverage_a_registration_covering_none_of_the_entity_types_is_not_matched() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let source = ContextSourceAdapter::start().await;
    let first = scope.entity_type("MultiTypeMissFirst");
    let second = scope.entity_type("MultiTypeMissSecond");
    let unrelated = scope.entity_type("MultiTypeMissUnrelated");

    let created = client
        .create_registration(&inclusive_update_registration(
            &scope,
            "registration:multitype-miss",
            &unrelated,
            &source.url("/"),
        ))
        .await
        .expect("create registration");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let entity_id = scope.id("entity:multitype-miss");
    let created = client
        .post_entity(&dual_typed_entity(&entity_id, &first, &second))
        .send()
        .await
        .expect("create entity");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let updated = client
        .update_entity_attrs(
            &entity_id,
            &json!({"speed": {"type": "Property", "value": 20}, "@context": [TEST_SUITE_CONTEXT]}),
        )
        .await
        .expect("update attributes");
    assert_eq!(
        updated.status,
        StatusCode::NO_CONTENT,
        "a purely local update of an unrelated-type entity: {:?}",
        updated.body
    );

    assert_eq!(
        source.get_total_count(),
        0,
        "a registration covering none of the entity's types must not be matched, so nothing is forwarded"
    );

    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}
