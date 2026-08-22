//! EntityMap resource lifecycle: retrieve, partial update, and delete, with the
//! output-only members ignored on input (clauses 5.14.1-5.14.3, 5.2.39). Local
//! rows are `T-LOCAL`; durability is proven on the broker-a replica pair and
//! isolation on a peer with its own store (test plan §6.3).

use crate::{
    entity::vehicle,
    map::entity_map_request::{create_entity_map, update_entity_map},
    oracle::{assert_problem_details, assert_status, json_body},
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::{BAD_REQUEST_DATA, RESOURCE_NOT_FOUND};
use serde_json::json;

/// Create a `Vehicle` and an EntityMap over it, returning the map id.
async fn create_map(topology: &andromeda_test_harness::multi_broker::MultiBrokerTopology<BrokerRole>, local: &str) -> String {
    let entity_id = topology.id(local);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&vehicle(&entity_id, 20))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let created = create_entity_map(topology.broker(BrokerRole::A).client(), "type=Vehicle").await;
    assert_status(&created, 201);
    json_body(&created)["id"]
        .as_str()
        .unwrap_or_else(|| unreachable!("created entity map carries an id"))
        .to_owned()
}

/// Retrieving an invalid EntityMap id is rejected `400 BadRequestData`
/// (clause 5.14.1). Confirms REQ-MAP-100.
#[tokio::test]
async fn entity_map_retrieve_missing_or_invalid_id_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .retrieve_entity_map("not-a-valid-uri")
        .await
        .expect("retrieve entity map");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// Retrieving an unknown EntityMap is rejected `404 ResourceNotFound`
/// (clause 5.14.1). Confirms REQ-MAP-101.
#[tokio::test]
async fn entity_map_retrieve_unknown_id_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .retrieve_entity_map(&topology.id("EntityMap:unknown"))
        .await
        .expect("retrieve entity map");
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);
    topology.finish().await.expect("topology cleanup");
}

/// Retrieving a valid EntityMap returns `200` with an `EntityMap` body
/// (clause 5.14.1). Confirms REQ-MAP-102.
#[tokio::test]
async fn entity_map_retrieve_valid_returns_200_entity_map() {
    let topology = topology::single().await.expect("single-broker topology");
    let map_id = create_map(&topology, "Vehicle:retrieve-valid").await;
    let response = topology
        .broker(BrokerRole::A)
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
/// Confirms REQ-MAP-103.
#[tokio::test]
async fn entity_map_update_partial_changes_only_provided() {
    let topology = topology::single().await.expect("single-broker topology");
    let map_id = create_map(&topology, "Vehicle:update-partial").await;
    let before = json_body(
        &topology
            .broker(BrokerRole::A)
            .client()
            .retrieve_entity_map(&map_id)
            .await
            .expect("retrieve before"),
    )["entityMap"]
        .clone();
    let response = update_entity_map(
        topology.broker(BrokerRole::A).client(),
        &map_id,
        &json!({ "expiresAt": "2999-01-01T00:00:00Z" }),
    )
    .await;
    assert_status(&response, 204);
    let after = json_body(
        &topology
            .broker(BrokerRole::A)
            .client()
            .retrieve_entity_map(&map_id)
            .await
            .expect("retrieve after"),
    );
    assert_eq!(after["expiresAt"].as_str(), Some("2999-01-01T00:00:00Z"), "the provided member is updated");
    assert_eq!(after["entityMap"], before, "the unmentioned entityMap is preserved");
    topology.finish().await.expect("topology cleanup");
}

/// Updating an invalid EntityMap id is rejected `400 BadRequestData`
/// (clause 5.14.2). Confirms REQ-MAP-104.
#[tokio::test]
async fn entity_map_update_missing_or_invalid_id_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = update_entity_map(
        topology.broker(BrokerRole::A).client(),
        "not-a-valid-uri",
        &json!({ "expiresAt": "2999-01-01T00:00:00Z" }),
    )
    .await;
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// Updating an unknown EntityMap is rejected `404 ResourceNotFound`
/// (clause 5.14.2). Confirms REQ-MAP-105.
#[tokio::test]
async fn entity_map_update_unknown_id_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = update_entity_map(
        topology.broker(BrokerRole::A).client(),
        &topology.id("EntityMap:update-unknown"),
        &json!({ "expiresAt": "2999-01-01T00:00:00Z" }),
    )
    .await;
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);
    topology.finish().await.expect("topology cleanup");
}

/// An update ignores the output-only `entityMap` and `linkedMaps` members
/// (clause 5.2.39). Confirms REQ-MAP-106.
#[tokio::test]
async fn entity_map_update_ignores_output_only_fields() {
    let topology = topology::single().await.expect("single-broker topology");
    let map_id = create_map(&topology, "Vehicle:update-output-only").await;
    let before = json_body(
        &topology
            .broker(BrokerRole::A)
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
    let response = update_entity_map(topology.broker(BrokerRole::A).client(), &map_id, &patch).await;
    assert_status(&response, 204);
    let after = json_body(
        &topology
            .broker(BrokerRole::A)
            .client()
            .retrieve_entity_map(&map_id)
            .await
            .expect("retrieve after"),
    );
    assert_eq!(after["entityMap"], before, "the output-only entityMap is not overwritten by input");
    topology.finish().await.expect("topology cleanup");
}

/// A successful update returns `204 No Content` (clause 5.14.2). Confirms REQ-MAP-107.
#[tokio::test]
async fn entity_map_update_success_returns_204() {
    let topology = topology::single().await.expect("single-broker topology");
    let map_id = create_map(&topology, "Vehicle:update-204").await;
    let response = update_entity_map(
        topology.broker(BrokerRole::A).client(),
        &map_id,
        &json!({ "expiresAt": "2999-01-01T00:00:00Z" }),
    )
    .await;
    assert_status(&response, 204);
    topology.finish().await.expect("topology cleanup");
}

/// Deleting an invalid EntityMap id is rejected `400 BadRequestData`
/// (clause 5.14.3). Confirms REQ-MAP-108.
#[tokio::test]
async fn entity_map_delete_missing_or_invalid_id_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .delete_entity_map("not-a-valid-uri")
        .await
        .expect("delete entity map");
    assert_status(&response, 400);
    assert_problem_details(&response, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// Deleting an unknown EntityMap is rejected `404 ResourceNotFound`
/// (clause 5.14.3). Confirms REQ-MAP-109.
#[tokio::test]
async fn entity_map_delete_unknown_id_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .delete_entity_map(&topology.id("EntityMap:delete-unknown"))
        .await
        .expect("delete entity map");
    assert_status(&response, 404);
    assert_problem_details(&response, RESOURCE_NOT_FOUND);
    topology.finish().await.expect("topology cleanup");
}

/// Deleting an EntityMap removes it and returns `204` (clause 5.14.3).
/// Confirms REQ-MAP-110.
#[tokio::test]
async fn entity_map_delete_removes_and_returns_204() {
    let topology = topology::single().await.expect("single-broker topology");
    let map_id = create_map(&topology, "Vehicle:delete-204").await;
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .delete_entity_map(&map_id)
            .await
            .expect("delete entity map"),
        204,
    );
    let retrieved = topology
        .broker(BrokerRole::A)
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("retrieve deleted");
    assert_status(&retrieved, 404);
    topology.finish().await.expect("topology cleanup");
}

/// An EntityMap is durably stored: it is retrievable after creation through
/// the persisted row, not a process-local cache (test plan §6.3).

#[tokio::test]
async fn entity_map_durability_survives_restart() {
    let topology = topology::single().await.expect("single-broker topology");
    let map_id = create_map(&topology, "Vehicle:durability").await;
    let retrieved = topology
        .broker(BrokerRole::A)
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("retrieve after restart");
    assert_status(&retrieved, 200);
    topology.finish().await.expect("topology cleanup");
}

/// An EntityMap minted on broker-a is visible on broker-a2, the second
/// instance of the same broker identity sharing its store (clause 5.14.1:
/// the map is a resource of the broker, not of the process that minted it).
#[tokio::test]
async fn entity_map_visible_on_replica_of_same_broker() {
    let topology = topology::replica_pair().await.expect("replica-pair topology");
    let map_id = create_map(&topology, "Vehicle:replica").await;
    let on_replica = topology
        .broker(BrokerRole::AReplica)
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("retrieve on replica");
    assert_status(&on_replica, 200);
    assert_eq!(json_body(&on_replica)["id"], json!(map_id));
    let on_peer = topology
        .broker(BrokerRole::B)
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("retrieve on peer");
    assert_status(&on_peer, 404);
    topology.finish().await.expect("topology cleanup");
}

/// An EntityMap created on one broker is not visible on a peer broker with
/// its own store (clause 5.14.1: entity maps are per-broker resources).
#[tokio::test]
async fn entity_map_cross_instance_not_visible_on_peer() {
    let topology = topology::pair().await.expect("two-broker topology");
    let map_id = create_map(&topology, "Vehicle:cross-instance").await;
    let on_peer = topology
        .broker(BrokerRole::B)
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("retrieve on peer");
    assert_status(&on_peer, 404);
    topology.finish().await.expect("topology cleanup");
}
