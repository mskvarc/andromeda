//! Registration delete lifecycle (clause 5.9.4): a missing or malformed id is
//! rejected, an unknown id is a 404, and a known registration is removed with a
//! 204 and no longer resolves. All `T-LOCAL`.

use crate::{
    oracle::{assert_problem_details, assert_status},
    reg::PEER_ENDPOINT,
    registration::csr_basic,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::{BAD_REQUEST_DATA, RESOURCE_NOT_FOUND};

/// A delete with no registration id targets the collection and is rejected with a
/// method/format error rather than succeeding (clause 5.9.4).
#[tokio::test]
async fn delete_missing_id_returns_400_or_405() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .delete_registration("")
        .await
        .expect("delete without id");
    let status = response.status.as_u16();
    assert!(matches!(status, 400 | 405), "deleting without an id is rejected, got {status}");

    topology.finish().await.expect("topology cleanup");
}

/// A delete against a syntactically invalid id is rejected (clause 5.9.4).
#[tokio::test]
async fn delete_invalid_id_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .delete_registration("not a uri")
        .await
        .expect("delete invalid id");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);

    topology.finish().await.expect("topology cleanup");
}

/// A delete against a well-formed but unknown registration id is a 404 (clause
/// 5.9.4).
#[tokio::test]
async fn delete_unknown_registration_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let unknown = topology.id("registration:delete-unknown");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .delete_registration(&unknown)
        .await
        .expect("delete unknown registration");
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);

    topology.finish().await.expect("topology cleanup");
}

/// A known registration is deleted with a 204 and no longer resolves (clause
/// 5.9.4).
#[tokio::test]
async fn delete_known_registration_returns_204() {
    let topology = topology::single().await.expect("single-broker topology");
    let id = topology.id("registration:delete-known");
    // A per-test entity type keeps this inclusive registration from matching a
    // concurrent test's Vehicle writes in the window before it is deleted.
    let mut body = csr_basic(&id, PEER_ENDPOINT);
    body["information"][0]["entities"][0]["type"] = serde_json::json!(topology.id("Vehicle"));
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&body)
        .await
        .expect("create registration");
    assert_status(&created, 201);

    let deleted = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .delete_registration(&id)
        .await
        .expect("delete registration");
    assert_status(&deleted, 204);
    assert!(deleted.body.is_empty(), "a successful delete returns an empty body");

    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_registration(&id, None)
        .await
        .expect("retrieve deleted registration");
    assert_status(&retrieved, 404);

    topology.finish().await.expect("topology cleanup");
}
