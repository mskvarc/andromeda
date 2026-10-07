//! A forwarded Temporal Evolution carries its reified Scope.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.5.6 makes the Scope of a Temporal Evolution
//! "a temporal representation of a Property", and clauses 5.6.11.4, 5.6.12.4
//! and 5.6.14.4 forward the operation's matching input data to the registered
//! Context Sources. A write whose `scope` is a history of instances must
//! therefore reach a source with every instance and the temporal members it
//! dated it with (`observedAt`, `deletedAt` and the Null clause 4.18 reserves
//! for a deleted scope), as the local write receives them. The `createdAt` and
//! `modifiedAt` of an instance are output only (clause 5.2.5, Table 5.2.5-2:
//! "In the event that they are provided (in update or create operations)
//! NGSI-LD implementations shall ignore them"), so the local write stamps its
//! own and a value a client sent is not among what is forwarded. The history is
//! forwarded, not only the paths of the Scope.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::context_source_adapter::ContextSourceAdapter;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::{assert_eq, assert_ne};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

/// The `createdAt` a client sends on the first Scope instance, which clause
/// 5.2.5 has implementations ignore.
const CLIENT_CREATED_AT: &str = "2025-01-01T00:00:01Z";

/// The `modifiedAt` a client sends on the first Scope instance, which clause
/// 5.2.5 has implementations ignore.
const CLIENT_MODIFIED_AT: &str = "2025-01-01T00:00:02Z";

/// An `inclusive` registration of this test's `covered_type` entities,
/// advertising `operation`.
fn temporal_registration(scope: &TestEnvironment, covered_type: &str, operation: &str, endpoint: &str) -> Value {
    json!({
        "id": scope.id("registration:reified-scope"),
        "type": "ContextSourceRegistration",
        "endpoint": endpoint,
        "mode": "inclusive",
        "operations": [operation],
        "information": [{
            "entities": [{"type": covered_type, "idPattern": format!("^{}:", scope.id_prefix())}]
        }],
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// The two-instance Scope history: a scope set, then ended with the NGSI-LD
/// Null beside its `deletedAt`.
fn reified_scope() -> Value {
    json!([
        {
            "type": "Property",
            "value": "/Madrid/Gardens",
            "observedAt": "2025-01-01T00:00:00Z",
            "createdAt": CLIENT_CREATED_AT,
            "modifiedAt": CLIENT_MODIFIED_AT
        },
        {
            "type": "Property",
            "value": "urn:ngsi-ld:null",
            "observedAt": "2025-01-02T00:00:00Z",
            "deletedAt": "2025-01-02T00:00:00Z"
        }
    ])
}

/// One observation of `speed`.
fn speed() -> Value {
    json!([{"type": "Property", "value": 10, "observedAt": "2025-01-01T00:00:00Z"}])
}

/// Assert that `body` carries both Scope instances with the temporal members
/// clause 4.5.6 lets a client date them with, and none of the output-only
/// `createdAt` and `modifiedAt` the client sent (clause 5.2.5).
fn assert_scope_history(body: &Value) {
    assert!(body["scope"].is_array(), "the Scope is forwarded as its history: {body}");
    let history = body["scope"].as_array().expect("the Scope was asserted to be an array");
    assert_eq!(history.len(), 2, "both Scope instances are forwarded: {body}");
    assert_eq!(history[0]["value"], json!("/Madrid/Gardens"), "{body}");
    assert_eq!(history[0]["observedAt"], json!("2025-01-01T00:00:00Z"), "{body}");
    assert_ne!(history[0]["createdAt"], json!(CLIENT_CREATED_AT), "{body}");
    assert_ne!(history[0]["modifiedAt"], json!(CLIENT_MODIFIED_AT), "{body}");
    assert_eq!(history[1]["value"], json!("urn:ngsi-ld:null"), "{body}");
    assert_eq!(history[1]["deletedAt"], json!("2025-01-02T00:00:00Z"), "{body}");
}

/// Register a source for `operation`, send `body` with `method` to `path` of the
/// local broker and return the body the source received.
async fn forwarded_body(scope: &TestEnvironment, covered_type: &str, operation: &str, method: Method, path: &str, body: &Value) -> Value {
    let client = scope.client();
    let source = ContextSourceAdapter::start().await;
    let registration = client
        .create_registration(&temporal_registration(scope, covered_type, operation, &source.url("/")))
        .await
        .expect("create registration");
    assert_eq!(registration.status, StatusCode::CREATED, "{:?}", registration.body);
    source.set_stub(method.clone(), path, 204).await;

    let response = client
        .raw(method.clone(), path)
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("the write is sent");
    assert!(
        response.status.is_success() || response.status == StatusCode::MULTI_STATUS,
        "{}",
        String::from_utf8_lossy(&response.body)
    );

    let forwarded = source.next_request(Duration::from_secs(5)).await;
    assert_eq!(forwarded.method, method);
    let received = forwarded.json::<Value>().expect("the forwarded body is JSON");
    source.shutdown().await;
    received
}

#[tokio::test]
async fn regression_temporal_reified_scope_forwarding_carries_every_scope_instance_to_the_source() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let covered_type = scope.entity_type("ReifiedScopeForwarding");
    let entity_id = scope.id("entity:reified-scope");
    let body = json!({
        "id": entity_id,
        "type": covered_type,
        "scope": reified_scope(),
        "speed": speed(),
        "@context": [TEST_SUITE_CONTEXT]
    });

    let received = forwarded_body(&scope, &covered_type, "upsertTemporal", Method::POST, "/ngsi-ld/v1/temporal/entities", &body).await;

    assert_scope_history(&received);
    assert!(received.get("speed").is_some(), "the instances travel beside the Scope: {received}");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.12.4: the Fragment of an add-attributes write is forwarded with
/// its Scope history.
#[tokio::test]
async fn regression_temporal_reified_scope_forwarding_add_attributes_carries_every_scope_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let covered_type = scope.entity_type("ReifiedScopeForwardingAttrs");
    let entity_id = scope.id("entity:reified-scope-attrs");
    let body = json!({"scope": reified_scope(), "speed": speed(), "@context": [TEST_SUITE_CONTEXT]});

    let received = forwarded_body(
        &scope,
        &covered_type,
        "appendAttrsTemporal",
        Method::POST,
        &format!("/ngsi-ld/v1/temporal/entities/{entity_id}/attrs"),
        &body,
    )
    .await;

    assert_scope_history(&received);
    assert!(received.get("speed").is_some(), "the instances travel beside the Scope: {received}");
    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.6.14.3 and 5.5.4: the Fragment of an update-attribute-instance
/// write includes "only the new Attribute instance", so a Scope history beside
/// it is refused with `BadRequestData` and nothing reaches a registered source.
#[tokio::test]
async fn regression_temporal_reified_scope_forwarding_update_instance_refuses_a_scope_beside_the_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let source = ContextSourceAdapter::start().await;
    let covered_type = scope.entity_type("ReifiedScopeForwardingInstance");
    let entity_id = scope.id("entity:reified-scope-instance");
    let instance_id = scope.id("instance:reified-scope");
    let path = format!("/ngsi-ld/v1/temporal/entities/{entity_id}/attrs/speed/{instance_id}");

    let registration = client
        .create_registration(&temporal_registration(&scope, &covered_type, "updateAttrs", &source.url("/")))
        .await
        .expect("create registration");
    assert_eq!(registration.status, StatusCode::CREATED, "{:?}", registration.body);
    source.set_stub(Method::PATCH, &path, 204).await;

    let response = client
        .raw(Method::PATCH, &path)
        .header("content-type", "application/ld+json")
        .json(&json!({"scope": reified_scope(), "speed": speed(), "@context": [TEST_SUITE_CONTEXT]}))
        .send()
        .await
        .expect("the write is sent");

    let problem: Value = serde_json::from_slice(&response.body).expect("a problem details body");
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{problem}");
    assert_eq!(problem["type"], json!("https://uri.etsi.org/ngsi-ld/errors/BadRequestData"), "{problem}");
    assert_eq!(source.get_total_count(), 0, "a refused write is not forwarded");

    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}
