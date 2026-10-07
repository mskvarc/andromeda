//! `EntityMap` resource lifecycle: retrieve, partial update, and delete, with the output-only members ignored on input (clauses 5.14.1-5.14.3, 5.2.39).

use crate::{
    entity::typed_vehicle,
    map::entity_map_request::{create_entity_map, update_entity_map},
    oracle::{assert_problem_details, assert_status, json_body},
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::{BAD_REQUEST_DATA, RESOURCE_NOT_FOUND};
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use pretty_assertions::assert_eq;
use serde_json::json;

/// Create a `Vehicle` under the topology's own prefix and an `EntityMap` over
/// it, returning the map id.
async fn create_map(topology: &MultiBrokerTopology<BrokerRole>, local: &str) -> String {
    create_map_over(topology, &topology.id(local)).await
}

/// Create the named `Vehicle` on broker A and an `EntityMap` over every
/// `Vehicle`, returning the map id.
async fn create_map_over(topology: &MultiBrokerTopology<BrokerRole>, entity_id: &str) -> String {
    // A per-test entity type keeps the bare `type=` map query from spanning
    // another concurrent test's Vehicles; the topology prefix makes it unique.
    let entity_type = topology.id("Vehicle");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(entity_id, &entity_type, 20))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let created = create_entity_map(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &format!("type={}", urlencoding::encode(&entity_type)),
    )
    .await;
    assert_status(&created, 201);
    json_body(&created)["id"].as_str().expect("created entity map carries an id").to_owned()
}

/// Retrieving an invalid `EntityMap` id is rejected `400 BadRequestData` (clause 5.14.1).
#[tokio::test]
async fn entity_map_retrieve_missing_or_invalid_id_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_entity_map("not-a-valid-uri")
        .await
        .expect("retrieve entity map");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// Retrieving an unknown `EntityMap` is rejected `404 ResourceNotFound` (clause 5.14.1).
#[tokio::test]
async fn entity_map_retrieve_unknown_id_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_entity_map(&topology.id("EntityMap:unknown"))
        .await
        .expect("retrieve entity map");
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);
    topology.finish().await.expect("topology cleanup");
}

/// Retrieving a valid `EntityMap` returns `200` with an `EntityMap` body (clause 5.14.1).
#[tokio::test]
async fn entity_map_retrieve_valid_returns_200_entity_map() {
    let topology = topology::single().await.expect("single-broker topology");
    let map_id = create_map(&topology, "Vehicle:retrieve-valid").await;
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("retrieve entity map");
    assert_status(&response, 200);
    let body = json_body(&response);
    assert_eq!(body["id"].as_str(), Some(map_id.as_str()), "the retrieved map carries its id");
    assert_eq!(body["type"].as_str(), Some("EntityMap"), "the retrieved resource is an EntityMap");
    assert!(body["entityMap"].is_object(), "the retrieved resource exposes an entityMap: {body}");
    topology.finish().await.expect("topology cleanup");
}

/// A partial update changes only the provided members (clause 5.14.2).
#[tokio::test]
async fn entity_map_update_partial_changes_only_provided() {
    let topology = topology::single().await.expect("single-broker topology");
    let map_id = create_map(&topology, "Vehicle:update-partial").await;
    let before = json_body(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .retrieve_entity_map(&map_id)
            .await
            .expect("retrieve before"),
    )["entityMap"]
        .clone();
    let response = update_entity_map(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &map_id,
        &json!({ "expiresAt": "2999-01-01T00:00:00Z" }),
    )
    .await;
    assert_status(&response, 204);
    let after = json_body(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .retrieve_entity_map(&map_id)
            .await
            .expect("retrieve after"),
    );
    assert_eq!(after["expiresAt"].as_str(), Some("2999-01-01T00:00:00Z"), "the provided member is updated");
    assert_eq!(after["entityMap"], before, "the unmentioned entityMap is preserved");
    topology.finish().await.expect("topology cleanup");
}

/// Updating an invalid `EntityMap` id is rejected `400 BadRequestData` (clause 5.14.2).
#[tokio::test]
async fn entity_map_update_missing_or_invalid_id_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = update_entity_map(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        "not-a-valid-uri",
        &json!({ "expiresAt": "2999-01-01T00:00:00Z" }),
    )
    .await;
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// Updating an unknown `EntityMap` is rejected `404 ResourceNotFound` (clause 5.14.2).
#[tokio::test]
async fn entity_map_update_unknown_id_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = update_entity_map(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &topology.id("EntityMap:update-unknown"),
        &json!({ "expiresAt": "2999-01-01T00:00:00Z" }),
    )
    .await;
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);
    topology.finish().await.expect("topology cleanup");
}

/// An update ignores the output-only `entityMap` and `linkedMaps` members (clause 5.2.39).
#[tokio::test]
async fn entity_map_update_ignores_output_only_fields() {
    let topology = topology::single().await.expect("single-broker topology");
    let map_id = create_map(&topology, "Vehicle:update-output-only").await;
    let before = json_body(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .retrieve_entity_map(&map_id)
            .await
            .expect("retrieve before"),
    )["entityMap"]
        .clone();
    let patch = json!({
        "expiresAt": "2999-01-01T00:00:00Z",
        "entityMap": { "urn:ngsi-ld:Vehicle:fake": ["@none"] },
        "linkedMaps": { "urn:ngsi-ld:csr:fake": "urn:ngsi-ld:EntityMap:fake" }
    });
    let response = update_entity_map(topology.broker(BrokerRole::A).expect("topology broker A").client(), &map_id, &patch).await;
    assert_status(&response, 204);
    let after = json_body(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .retrieve_entity_map(&map_id)
            .await
            .expect("retrieve after"),
    );
    assert_eq!(after["entityMap"], before, "the output-only entityMap is not overwritten by input");
    topology.finish().await.expect("topology cleanup");
}

/// A successful update returns `204 No Content` (clause 5.14.2).
#[tokio::test]
async fn entity_map_update_success_returns_204() {
    let topology = topology::single().await.expect("single-broker topology");
    let map_id = create_map(&topology, "Vehicle:update-204").await;
    let response = update_entity_map(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &map_id,
        &json!({ "expiresAt": "2999-01-01T00:00:00Z" }),
    )
    .await;
    assert_status(&response, 204);
    topology.finish().await.expect("topology cleanup");
}

/// Deleting an invalid `EntityMap` id is rejected `400 BadRequestData` (clause 5.14.3).
#[tokio::test]
async fn entity_map_delete_missing_or_invalid_id_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .delete_entity_map("not-a-valid-uri")
        .await
        .expect("delete entity map");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// Deleting an unknown `EntityMap` is rejected `404 ResourceNotFound` (clause 5.14.3).
#[tokio::test]
async fn entity_map_delete_unknown_id_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .delete_entity_map(&topology.id("EntityMap:delete-unknown"))
        .await
        .expect("delete entity map");
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);
    topology.finish().await.expect("topology cleanup");
}

/// Deleting an `EntityMap` removes it and returns `204` (clause 5.14.3).
#[tokio::test]
async fn entity_map_delete_removes_and_returns_204() {
    let topology = topology::single().await.expect("single-broker topology");
    let map_id = create_map(&topology, "Vehicle:delete-204").await;
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .delete_entity_map(&map_id)
            .await
            .expect("delete entity map"),
        204,
    );
    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("retrieve deleted");
    assert_status(&retrieved, 404);
    topology.finish().await.expect("topology cleanup");
}

/// Broker-a2 is a second instance of the broker-a identity over the same store, so a read there cannot be answered from broker-a's process state. Retrieving the map on the instance that created it proves nothing about durability at all: an entirely in-memory map answers that request too.
#[tokio::test]
async fn entity_map_contents_survive_the_process_that_minted_them() {
    let topology = topology::replica_pair().await.expect("replica-pair topology");
    let entity_id = topology.id("Vehicle:durability");
    let map_id = create_map_over(&topology, &entity_id).await;
    let on_replica = topology
        .broker(BrokerRole::AReplica)
        .expect("topology broker A replica")
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("retrieve on the second instance");
    assert_status(&on_replica, 200);
    let map = json_body(&on_replica)["entityMap"].clone();
    assert!(
        map.get(&entity_id).is_some(),
        "the persisted row carries the candidate set, not just the map id: {map}"
    );
    topology.finish().await.expect("topology cleanup");
}

/// An `EntityMap` minted on broker-a is visible on broker-a2, the second instance of the same broker identity sharing its store (clause 5.14.1: the map is a resource of the broker, not of the process that minted it).
#[tokio::test]
async fn entity_map_visible_on_replica_of_same_broker() {
    let topology = topology::replica_pair().await.expect("replica-pair topology");
    let map_id = create_map(&topology, "Vehicle:replica").await;
    let on_replica = topology
        .broker(BrokerRole::AReplica)
        .expect("topology broker A replica")
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("retrieve on replica");
    assert_status(&on_replica, 200);
    assert_eq!(json_body(&on_replica)["id"], json!(map_id));
    let on_peer = topology
        .broker(BrokerRole::B)
        .expect("topology broker B")
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("retrieve on peer");
    assert_status(&on_peer, 404);
    topology.finish().await.expect("topology cleanup");
}

/// An `EntityMap` created on one broker is not visible on a peer broker with its own store (clause 5.14.1: entity maps are per-broker resources).
#[tokio::test]
async fn entity_map_cross_instance_not_visible_on_peer() {
    let topology = topology::pair().await.expect("two-broker topology");
    let map_id = create_map(&topology, "Vehicle:cross-instance").await;
    let on_peer = topology
        .broker(BrokerRole::B)
        .expect("topology broker B")
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("retrieve on peer");
    assert_status(&on_peer, 404);
    topology.finish().await.expect("topology cleanup");
}
