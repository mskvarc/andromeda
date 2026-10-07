//! A subscription's `jsonldContext` wrapper is served even after another replica deleted it.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.8.1.4 initialises a subscription's `jsonldContext` with the `@context` applicable for it, and Table 5.2.12-1 makes that one URI, so a compound `@context` is named through an `ImplicitlyCreated` wrapper, which clause 5.13.1 says "shall be served by brokers on-demand". Clause 5.13.5.4 lets a client delete any stored entry, a wrapper included. Equal wrappers share one entry under an identifier their content derives, so a broker must not trust a memory of which identifiers it stored: a wrapper deleted through one replica must be stored again by the other, or a subscription would name a URL nothing serves.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::large_entity_replicas::second_replica;
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// A subscription whose `@context` no single URL names, so its `jsonldContext` is a wrapper the broker hosts (Annex C.8).
fn compound_context_subscription(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Sensor"}],
        "notification": {"endpoint": {"uri": "http://notified.example/sink", "accept": "application/json"}},
        "@context": [{"reading": "https://example.org/terms/reading"}, CORE_CONTEXT]
    })
}

/// Create the subscription `id` through `client` and answer the
/// `jsonldContext` the broker initialised it with.
async fn subscribe(client: &TestClient, id: &str) -> String {
    let created = client
        .create_subscription(&compound_context_subscription(id))
        .await
        .expect("create subscription request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let retrieved = client.retrieve_subscription(id, None).await.expect("retrieve subscription request");
    assert_eq!(retrieved.status, StatusCode::OK, "{:?}", retrieved.body);
    retrieved.json_value().expect("a subscription body")["jsonldContext"]
        .as_str()
        .expect("clause 5.8.1.4 initialises jsonldContext")
        .to_owned()
}

/// Clause 6.3.14 makes the `NGSILD-Tenant` header the only way a request names a tenant, so the wrapper a non-default tenant stored is dereferenced under that header.
async fn dereference(url: &str, tenant: &str) -> StatusCode {
    reqwest::Client::new()
        .get(url)
        .header("NGSILD-Tenant", tenant)
        .send()
        .await
        .expect("the wrapper URL is reachable")
        .status()
}

#[tokio::test]
async fn regression_implicit_context_a_wrapper_deleted_through_another_replica_is_stored_again_for_a_subscription() {
    let scope = TenantScopedEnvironment::from_suite("regression").await.expect("private tenant environment");
    let first = scope.client();
    let second = second_replica(&scope);
    let seeded = first
        .post_entity(&json!({"id": scope.id("Sensor:seed"), "type": "Sensor", "@context": CORE_CONTEXT}))
        .send()
        .await
        .expect("seed entity request");
    assert_eq!(seeded.status, StatusCode::CREATED, "the seed brings the tenant into being: {:?}", seeded.body);

    let wrapper = subscribe(first, &scope.id("Subscription:before-deletion")).await;
    assert_eq!(
        dereference(&wrapper, scope.tenant()).await,
        StatusCode::OK,
        "clause 5.13.1 serves the wrapper: {wrapper}"
    );

    let identifier = wrapper.rsplit('/').next().expect("the wrapper URL ends in its identifier");
    let deleted = second.delete_jsonld_context(identifier).await.expect("delete @context request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "clause 5.13.5.4 removes the entry: {:?}", deleted.body);
    assert_eq!(
        dereference(&wrapper, scope.tenant()).await,
        StatusCode::NOT_FOUND,
        "the deleted wrapper is gone"
    );

    let renamed = subscribe(first, &scope.id("Subscription:after-deletion")).await;
    assert_eq!(renamed, wrapper, "equal wrappers share the identifier their content derives");
    assert_eq!(
        dereference(&renamed, scope.tenant()).await,
        StatusCode::OK,
        "a subscription created after the deletion names a wrapper that is served: {renamed}"
    );

    scope.finish().await.expect("cleanup failed");
}
