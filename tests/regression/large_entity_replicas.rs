//! Two replicas over one database budget, serve and delete large entities
//! alike.
//!
//! Several broker replicas sharing one database are a supported deployment,
//! and a load balancer sends any request to any of them. Each replica budgets
//! what it admits from its own configuration, which the replicas of one
//! deployment share, so both refuse an oversized geometry alike (clause 6.3.2),
//! and an entity one replica wrote is read and deleted whole by the other. A
//! deletion reads the state it removes only when a subscription may be owed its
//! notification; the replica serving it asks the shared database whether one
//! may, so a subscription another replica created is notified (clause 5.8.6).
//!
//! The second replica is the base tier's `broker-trusted`, which serves the
//! same database as `broker`.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::{
    large_entity_budget::{
        CORE_CONTEXT,
        GEOMETRY_POSITIONS,
        REQUEST_ENTITY_TOO_LARGE,
        assert_absent,
        assert_problem,
        located_entity,
        located_entity_in_state,
        location_ring,
        polygon_text,
        ring,
        send_json,
        subscribe_to_deletions,
    },
    notification_adapter::NotificationAdapter,
};
use andromeda_test_client::{broker_endpoint::BrokerEndpoint, tenant_name::TenantName, test_client::TestClient};
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::json;
use std::{env::var, time::Duration};

/// Names the second replica of the tier, which serves the same database.
const SECOND_BROKER_ENV: &str = "ANDROMEDA_TEST_TRUSTED_PROXY_BROKER_URL";

/// The second replica's API root when [`SECOND_BROKER_ENV`] names no other.
const DEFAULT_SECOND_BROKER: &str = "http://127.0.0.1:1057/ngsi-ld/v1/";

/// How long the owed deletion notification is waited for.
const ARRIVAL: Duration = Duration::from_secs(30);

/// How long the absence of a second notification is watched for.
const QUIET: Duration = Duration::from_secs(8);

/// A client of the second replica bound to the tenant of `scope`.
pub(crate) fn second_replica(scope: &TenantScopedEnvironment) -> TestClient {
    let root = var(SECOND_BROKER_ENV).unwrap_or_else(|_| DEFAULT_SECOND_BROKER.to_owned());
    let endpoint = BrokerEndpoint::parse(root).expect("the second replica's URL parses");
    TestClient::new(endpoint, Some(TenantName::parse(scope.tenant()).expect("the private tenant is a tenant name")))
}

/// Create a one-attribute entity of `entity_type` through `client`, which
/// brings the private tenant into being.
async fn create_small(client: &TestClient, id: &str, entity_type: &str) {
    let created = client
        .post_entity(&json!({"id": id, "type": entity_type, "n": {"type": "Property", "value": 1}, "@context": CORE_CONTEXT}))
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
}

#[tokio::test]
async fn regression_a_n12_budgets_refuse_alike_on_both_replicas() {
    let scope = TenantScopedEnvironment::from_suite("regression").await.expect("private tenant environment");
    let first = scope.client();
    let second = second_replica(&scope);
    let entity_type = "Probe";
    create_small(first, &scope.id("Probe:tenant"), entity_type).await;

    let oversized = polygon_text(&ring(GEOMETRY_POSITIONS + 1));
    for (replica, local) in [(first, "Probe:over-first"), (&second, "Probe:over-second")] {
        let id = scope.id(local);
        let refused = send_json(replica, Method::POST, "/ngsi-ld/v1/entities", located_entity(&id, entity_type, &oversized)).await;
        assert_problem(
            &refused,
            StatusCode::PAYLOAD_TOO_LARGE,
            REQUEST_ENTITY_TOO_LARGE,
            &["location", &GEOMETRY_POSITIONS.to_string()],
        );
    }

    let id = scope.id("Probe:at-budget");
    let sent = ring(GEOMETRY_POSITIONS);
    let created = send_json(
        first,
        Method::POST,
        "/ngsi-ld/v1/entities",
        located_entity(&id, entity_type, &polygon_text(&sent)),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let read = second.get_entity(&id, "", "application/json", None).await.expect("retrieve entity request");
    assert_eq!(read.status, StatusCode::OK);
    assert!(
        location_ring(&read.json_value().expect("an entity")) == sent,
        "the other replica reads the ring sent"
    );
    let deleted = second.delete_entity(&id).await.expect("delete entity request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);
    assert_absent(first, &id).await;
    scope.finish().await.expect("cleanup");
}

#[tokio::test]
async fn regression_a_n12_a_deletion_on_one_replica_notifies_with_the_removed_state() {
    let scope = TenantScopedEnvironment::from_suite("regression").await.expect("private tenant environment");
    let adapter = NotificationAdapter::start().await;
    let first = scope.client();
    let second = second_replica(&scope);
    let entity_type = "Probe";
    // The tenant comes into being through the first replica, before the second
    // has served it, so the second loads its subscriptions only after the
    // subscription below exists.
    create_small(first, &scope.id("Probe:tenant"), entity_type).await;
    let subscription = scope.id("Subscription:a-n12-replica-deletion");
    let failing = scope.id("Subscription:a-n12-replica-deletion-active");
    subscribe_to_deletions(first, &adapter, &subscription, entity_type, &["entityDeleted"], "state==\"retired\"").await;
    subscribe_to_deletions(first, &adapter, &failing, entity_type, &["entityDeleted"], "state==\"active\"").await;
    let served = second
        .query_entities(&format!("type={entity_type}"), "application/json", None)
        .await
        .expect("query entities request");
    assert_eq!(served.status, StatusCode::OK, "{:?}", served.body);

    let id = scope.id("Probe:replica-deleted");
    let created = send_json(
        first,
        Method::POST,
        "/ngsi-ld/v1/entities",
        located_entity_in_state(&id, entity_type, &polygon_text(&ring(10_000)), "retired"),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let deleted = second.delete_entity(&id).await.expect("delete entity request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);

    // Only the removed state tells the two subscriptions apart (clause 5.8.6).
    let notification = adapter.wait_for_subscription_notification(&subscription, ARRIVAL).await;
    adapter.wait_for_no_subscription_notification(&subscription, QUIET).await;
    adapter.wait_for_no_subscription_notification(&failing, QUIET).await;
    assert_eq!(notification.body["data"][0]["id"], id.as_str(), "{}", notification.body);
    assert!(notification.body["data"][0].get("deletedAt").is_some(), "{}", notification.body);
    adapter.shutdown().await;
    scope.finish().await.expect("cleanup");
}
