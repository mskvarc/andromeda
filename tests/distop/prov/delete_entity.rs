//! Delete Entity forwarding — REQ-PROV-41..46 (clause 5.6.6.4).
//!
//! A delete with no local entity and no matching registration is `404`, but a
//! matching registration makes the delete forwardable rather than not-found.
//! Exclusive/redirect registrations forward the delete and terminate; an
//! unadvertised `deleteEntity` yields a bare `409 Conflict`; inclusive forwards
//! and also removes the local entity; and a `T-PAIR` row proves the redirect
//! delete reaches real broker B.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property},
    oracle::{assert_problem_details, assert_status},
    prov::forwarding::{FORWARD_WAIT, request_path},
    registration::CsrBuilder,
    topology::{BrokerRole, DistributedTopology},
};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::problem_detail::{CONFLICT, RESOURCE_NOT_FOUND};
use axum::http::Method;
use serde_json::{Value, json};

/// A `Vehicle` carrying a `brandName` and a `speed`.
fn brand_and_speed(id: &str) -> Value {
    entity(id, "Vehicle", &[("brandName", property(json!("Mercedes"))), ("speed", property(json!(42)))])
}

/// Seed an entity purely locally, bypassing forwarding.
async fn seed_local(client: &TestClient, payload: &Value) {
    let seeded = client
        .raw(Method::POST, "/ngsi-ld/v1/entities?local=true")
        .header("content-type", "application/ld+json")
        .json(payload)
        .send()
        .await
        .expect("seed local entity");
    assert_status(&seeded, 201);
}

/// A delete for an absent entity with no matching registration is
/// `404 ResourceNotFound`. Confirms REQ-PROV-41 (positive).
#[tokio::test]
async fn delete_entity_absent_and_no_registration_returns_404() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let entity_id = topology.id("Vehicle:prov-delete-absent");

    let deleted = client.delete_entity(&entity_id).await.expect("delete entity");
    assert_status(&deleted, 404);
    assert_problem_details(&deleted, RESOURCE_NOT_FOUND);

    topology.finish().await.expect("topology cleanup");
}

/// A delete for an absent entity that a registration matches is forwarded rather
/// than answered `404`. Confirms REQ-PROV-41 (negative).
#[tokio::test]
async fn delete_entity_registration_matches_entity_absent_not_404() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-delete-match-absent");
    let reg_id = topology.id("registration:prov-delete-match-absent");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .entity_id(&entity_id)
        .operations(&["deleteEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let forward_path = format!("/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::DELETE, &forward_path, 204).await;

    let deleted = client.delete_entity(&entity_id).await.expect("delete entity");
    assert!(
        deleted.status.as_u16() != 404,
        "a matching registration forwards rather than 404: {}",
        String::from_utf8_lossy(&deleted.body)
    );
    assert_eq!(mock.get_count(Method::DELETE, &forward_path).await, 1, "the delete is forwarded once");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A redirect registration forwards the delete and terminates local processing.
/// The registration precedes the local seed because clause 5.9.2.4 rejects a
/// redirect registration when "an existing Entity already matches the Context
/// Source Registration". Confirms REQ-PROV-42, 43.
#[tokio::test]
async fn delete_entity_exclusive_redirect_match_forwards() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-delete-redirect");
    let reg_id = topology.id("registration:prov-delete-redirect");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .entity_id(&entity_id)
        .operations(&["deleteEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    seed_local(client, &brand_and_speed(&entity_id)).await;
    let forward_path = format!("/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::DELETE, &forward_path, 204).await;

    let deleted = client.delete_entity(&entity_id).await.expect("delete entity");
    assert_status(&deleted, 204);

    let forwarded = mock.next_request(FORWARD_WAIT).await;
    assert!(
        request_path(&forwarded).ends_with(entity_id.as_str()),
        "the delete forward targets the entity: {}",
        request_path(&forwarded)
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A redirect registration that does not advertise `deleteEntity` rejects the
/// delete with a bare `409 Conflict` and no forwarding. No local copy is seeded:
/// clause 5.6.6.4 answers `Conflict` only "in case the complete delete Entity
/// failed" and a partial success otherwise, so a locally deletable copy would
/// turn the row into a `207`. Confirms REQ-PROV-44 (complete-fail).
#[tokio::test]
async fn delete_entity_op_unsupported_returns_409() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-delete-unsup");
    let reg_id = topology.id("registration:prov-delete-unsup");

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("redirect")
        .entity_id(&entity_id)
        .operations(&["retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let forward_path = format!("/ngsi-ld/v1/entities/{entity_id}");

    let deleted = client.delete_entity(&entity_id).await.expect("delete entity");
    assert_status(&deleted, 409);
    assert_problem_details(&deleted, CONFLICT);
    assert_eq!(mock.get_count(Method::DELETE, &forward_path).await, 0, "no forward on an unsupported operation");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An inclusive registration forwards the delete and also removes the local
/// entity. Confirms REQ-PROV-45, 46.
#[tokio::test]
async fn delete_entity_inclusive_forwards_and_removes_local() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let client = topology.front().client();
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:prov-delete-inclusive");
    let reg_id = topology.id("registration:prov-delete-inclusive");

    seed_local(client, &brand_and_speed(&entity_id)).await;

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["deleteEntity", "retrieveEntity"])
        .build();
    assert_status(&client.create_registration(&registration).await.expect("create registration"), 201);
    let forward_path = format!("/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub(Method::DELETE, &forward_path, 204).await;

    let deleted = client.delete_entity(&entity_id).await.expect("delete entity");
    assert_status(&deleted, 204);
    assert_eq!(mock.get_count(Method::DELETE, &forward_path).await, 1, "the delete is forwarded once");

    let local = client
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local read-back");
    assert_status(&local, 404);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// End-to-end against real broker B: a redirect delete on A removes the entity on
/// B. Confirms REQ-PROV-42, 43.
#[tokio::test]
async fn delete_entity_pair_redirect_end_to_end() {
    let topology = DistributedTopology::pair().await.expect("two-broker topology");
    let front = topology.front();
    let peer = topology.broker(BrokerRole::B);
    let entity_id = topology.id("Vehicle:prov-delete-pair");
    let reg_id = topology.id("registration:prov-delete-pair");

    let created = peer
        .client()
        .post_entity(&brand_and_speed(&entity_id))
        .with_ld_json()
        .send()
        .await
        .expect("peer create");
    assert_status(&created, 201);

    let registration = CsrBuilder::new(&reg_id, peer.peer_url())
        .mode("redirect")
        .entity_id(&entity_id)
        .operations(&["deleteEntity", "retrieveEntity"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("create registration"), 201);

    let deleted = front.client().delete_entity(&entity_id).await.expect("delete entity");
    assert_status(&deleted, 204);

    let peer_local = peer
        .client()
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("peer read-back");
    assert_status(&peer_local, 404);

    topology.finish().await.expect("topology cleanup");
}
