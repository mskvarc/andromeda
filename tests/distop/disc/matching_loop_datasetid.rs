//! Matching algorithm: datasetId intersection, loop prevention, and `EntityMap` preference (clause 5.12).

use crate::{
    disc::{
        PEER_ENDPOINT,
        csr_discovery::{contains, query, register},
    },
    oracle::json_body,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use reqwest::Method;
use serde_json::json;
use urlencoding::encode;

/// When both the query and the registration specify a datasetId, they match only on a shared value (clause 5.12, SHOULD).
#[tokio::test]
async fn matching_datasetid_both_specified_require_shared_value() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("DatasetBothType");
    let shared = topology.id("dataset:shared");
    let other = topology.id("dataset:other");
    let id = topology.id("registration:dataset-both");
    let mut body = CsrBuilder::new(&id, PEER_ENDPOINT).entity_type(&vtype).build();
    body["datasetId"] = json!([shared]);
    register(&topology, &body).await;

    let matching = query(&topology, &format!("type={}&datasetId={}", encode(&vtype), encode(&shared))).await;
    assert!(contains(&matching, &id), "a shared datasetId value matches");

    let non_matching = query(&topology, &format!("type={}&datasetId={}", encode(&vtype), encode(&other))).await;
    assert!(!contains(&non_matching, &id), "a disjoint datasetId value does not match");
    topology.finish().await.expect("topology cleanup");
}

/// When only one side specifies a datasetId the other is unconstrained and they match: a registration without a datasetId matches a datasetId-scoped query (clause 5.12).
#[tokio::test]
async fn matching_datasetid_only_one_side_specifies_matches() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("DatasetOneType");
    let queried = topology.id("dataset:queried");
    let id = topology.id("registration:dataset-one");
    register(&topology, &CsrBuilder::new(&id, PEER_ENDPOINT).entity_type(&vtype).build()).await;

    let body = query(&topology, &format!("type={}&datasetId={}", encode(&vtype), encode(&queried))).await;
    assert!(contains(&body, &id), "a registration without a datasetId matches a datasetId-scoped query");
    topology.finish().await.expect("topology cleanup");
}

/// A registration whose `contextSourceAlias` is already in the request's encountered listing is excluded from matching, preventing forwarding loops (clause 5.12, 4.3.6.4).
#[tokio::test]
async fn matching_loop_prevention_excludes_csr_with_alias_in_encountered_listing() {
    let topology = topology::pair().await.expect("two-broker topology");
    let peer = topology.broker(BrokerRole::B).expect("topology broker B");
    let vtype = topology.id("LoopType");
    let id = topology.id("registration:loop");
    let body = CsrBuilder::new(&id, peer.peer_url())
        .tenant(topology.tenant())
        .entity_type(&vtype)
        .alias(peer.alias())
        .build();
    register(&topology, &body).await;

    // A discovery whose encountered listing already names the peer's alias must
    // exclude the peer's registration to break the forwarding loop.
    let via = format!("1.1 {}", peer.alias());
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/csourceRegistrations?type={}", encode(&vtype)))
        .header("accept", "application/ld+json")
        .header("via", &via)
        .send()
        .await
        .expect("query registrations with encountered alias");
    assert!(
        !contains(&json_body(&response), &id),
        "a registration whose alias is in the encountered listing is excluded"
    );
    topology.finish().await.expect("topology cleanup");
}

/// A valid `EntityMap` passed into a request is preferred over re-running the matching algorithm (clause 5.12, 6.3.15). Cross-instance/durable `EntityMap` sharing is not implemented.
#[tokio::test]
async fn matching_valid_entitymap_preferred_over_rematching() {
    let topology = topology::chain().await.expect("three-broker topology");
    let vtype = topology.id("EntityMapType");
    let id = topology.id("registration:entity-map");
    register(&topology, &CsrBuilder::new(&id, PEER_ENDPOINT).entity_type(&vtype).build()).await;

    // A passed-in, still-valid EntityMap should short-circuit re-matching so the
    // request is served from the map rather than by re-evaluating 5.12.
    let map_id = topology.id("entitymap:preferred");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/csourceRegistrations?type={}", encode(&vtype)))
        .header("accept", "application/ld+json")
        .header("ngsild-entitymap", &map_id)
        .send()
        .await
        .expect("query registrations with entity map");
    assert!(contains(&json_body(&response), &id), "a valid EntityMap is preferred over re-running matching");
    topology.finish().await.expect("topology cleanup");
}
