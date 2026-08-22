//! Temporal delete-evolution forwarding — REQ-PROV-104..107 (clause 5.6.16.4).
//! The advertised operation is the Table 4.20-1 name `deleteTemporal`.
//!
//! Deleting a temporal entity's whole evolution reports `404` only when nothing
//! matches, forwards to the owning source and terminates for exclusive
//! registrations, and forwards while removing the local evolution for inclusive
//! registrations.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    oracle::{assert_problem_details, assert_status},
    prov::{
        forwarding::{FORWARD_WAIT, request_path},
        temporal_payloads::temporal_vehicle,
    },
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::problem_detail::RESOURCE_NOT_FOUND;
use axum::http::Method;
use serde_json::Value;

/// Seed a purely-local temporal entity, bypassing forwarding.
async fn seed_local_temporal(client: &TestClient, body: &Value) {
    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/temporal/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("seed local temporal");
    assert!(
        matches!(seeded.status.as_u16(), 201 | 204),
        "local temporal seed succeeds: {}",
        String::from_utf8_lossy(&seeded.body)
    );
}

/// Deleting the evolution of an absent entity with no matching registration is
/// `404 ResourceNotFound`. Confirms REQ-PROV-104 (positive).
#[tokio::test]
async fn temporal_delete_evolution_absent_and_no_registration_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).client();
    let entity_id = topology.id("Vehicle:prov-tdelevo-absent");

    let response = client.delete_temporal_entity(&entity_id).await.expect("delete temporal entity");
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);

    topology.finish().await.expect("topology cleanup");
}

/// Exclusive delete-evolution forwards to the owning source and terminates local
/// processing. Confirms REQ-PROV-104 (negative), 105, 106.
#[tokio::test]
async fn temporal_delete_evolution_exclusive_match_forwards_and_terminates() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-tdelevo-excl");
    let reg_id = topology.id("registration:prov-tdelevo-excl");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["deleteTemporal", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    seed_local_temporal(client, &temporal_vehicle(&entity_id)).await;
    let evolution_path = format!("/ngsi-ld/v1/temporal/entities/{entity_id}");
    mock.set_stub(Method::DELETE, &evolution_path, 204).await;

    let response = client.delete_temporal_entity(&entity_id).await.expect("delete temporal entity");
    assert!(
        matches!(response.status.as_u16(), 200 | 204 | 207),
        "exclusive delete-evolution succeeds: {}",
        String::from_utf8_lossy(&response.body)
    );

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    assert!(
        request_path(&forwarded).ends_with(entity_id.as_str()),
        "the delete forwards to the owning source: {}",
        request_path(&forwarded)
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Inclusive delete-evolution forwards the delete and removes the local
/// evolution. Confirms REQ-PROV-107.
#[tokio::test]
async fn temporal_delete_evolution_inclusive_forwards_and_removes_local() {
    let topology = topology::single().await.expect("single-broker topology");
    let client = topology.broker(BrokerRole::A).client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-tdelevo-incl");
    let reg_id = topology.id("registration:prov-tdelevo-incl");

    seed_local_temporal(client, &temporal_vehicle(&entity_id)).await;

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["deleteTemporal", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let evolution_path = format!("/ngsi-ld/v1/temporal/entities/{entity_id}");
    mock.set_stub(Method::DELETE, &evolution_path, 204).await;

    let response = client.delete_temporal_entity(&entity_id).await.expect("delete temporal entity");
    assert!(
        matches!(response.status.as_u16(), 200 | 204 | 207),
        "inclusive delete-evolution succeeds: {}",
        String::from_utf8_lossy(&response.body)
    );
    assert_eq!(mock.get_count(Method::DELETE, &evolution_path).await, 1, "the delete is forwarded once");

    let local = client.get_temporal_entity(&entity_id, "local=true", None).await.expect("local read-back");
    assert_status(&local, 404);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
