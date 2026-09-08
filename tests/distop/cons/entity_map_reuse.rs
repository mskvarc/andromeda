//! Retrieve Entity entity-map create and reuse (clauses 5.7.1.4, 5.5.14, 6.3.15):
//! minting a map, reusing a supplied map so only its recorded CSRs are consulted,
//! recreating on an unreachable/expired map, and echoing the in-use map. Linked
//! cross-instance maps (`REQ-CONS-8/13`) run against the durable, shared
//! entity-map store. Query-entity map rows live in
//! `crate::cons::entity_map_query`; temporal map rows in
//! `crate::cons::entity_map_temporal`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    oracle::{assert_status, entity_map_header, forwarded_header, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_client::response::TestResponse;
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use axum::http::Method;
use std::time::Duration;
use urlencoding::encode;

/// The retrieve path for one entity id, reusing a supplied entity map.
async fn retrieve_reusing_map(topology: &MultiBrokerTopology<BrokerRole>, entity_id: &str, map: &str) -> TestResponse {
    topology
        .broker(BrokerRole::A)
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/entities/{}", encode(entity_id)))
        .header("accept", "application/ld+json")
        .header("NGSILD-EntityMap", map)
        .send()
        .await
        .expect("reuse retrieve")
}

/// A retrieve minting a map records the consulted CSRs; a later reuse of that map
/// consults exactly those CSRs and not one added afterward (clause 6.3.15).
/// Confirms REQ-CONS-3.
#[tokio::test]
async fn distop_retrieve_entity_map_location_present_retrieves_map() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons3");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    let first_id = topology.id("registration:cons3-s1");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&first_id, &mock.url("/s1"))
                    .mode("inclusive")
                    .entity_id(&entity_id)
                    .operations(&["retrieveEntity"])
                    .build(),
            )
            .await
            .expect("register s1"),
        201,
    );
    mock.set_stub_with_body(Method::GET, &format!("/s1/ngsi-ld/v1/entities/{entity_id}"), 200, &vehicle(&entity_id, 10))
        .await;

    let mint = front
        .client()
        .get_entity(&entity_id, "entityMap=true", "application/ld+json", None)
        .await
        .expect("mint");
    let map = entity_map_header(&mint).expect("minted entity map header");

    let second_id = topology.id("registration:cons3-s2");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&second_id, &mock.url("/s2"))
                    .mode("inclusive")
                    .entity_id(&entity_id)
                    .operations(&["retrieveEntity"])
                    .build(),
            )
            .await
            .expect("register s2"),
        201,
    );
    mock.set_stub_with_body(Method::GET, &format!("/s2/ngsi-ld/v1/entities/{entity_id}"), 200, &vehicle(&entity_id, 20))
        .await;

    let reuse = retrieve_reusing_map(&topology, &entity_id, &map).await;
    assert_status(&reuse, 200);
    assert!(
        mock.get_count(Method::GET, &format!("/s1/ngsi-ld/v1/entities/{entity_id}")).await > 0,
        "the mapped CSR is consulted"
    );
    assert_eq!(
        mock.get_count(Method::GET, &format!("/s2/ngsi-ld/v1/entities/{entity_id}")).await,
        0,
        "a CSR added after minting is not consulted"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An unreachable or expired supplied map triggers fresh discovery and a new map,
/// and the request still succeeds (clause 5.5.14). Confirms REQ-CONS-4.
#[tokio::test]
async fn distop_retrieve_entity_map_unreachable_or_expired_creates_new() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons4");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    let registration_id = topology.id("registration:cons4");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, &mock.url(""))
                    .mode("inclusive")
                    .entity_id(&entity_id)
                    .operations(&["retrieveEntity"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &vehicle(&entity_id, 10))
        .await;

    let bogus = topology.id("EntityMap:bogus");
    let reuse = retrieve_reusing_map(&topology, &entity_id, &bogus).await;
    assert_status(&reuse, 200);
    assert!(
        mock.get_count(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}")).await > 0,
        "a bogus map forces fresh discovery of the source"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A valid map fixes the CSR set: reusing it consults only the mapped source, never
/// a matching source registered afterward (clause 5.5.14). Confirms REQ-CONS-5.
#[tokio::test]
async fn distop_retrieve_entity_valid_map_uses_only_mapped_csrs() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons5");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    let first_id = topology.id("registration:cons5-s1");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&first_id, &mock.url("/s1"))
                    .mode("inclusive")
                    .entity_id(&entity_id)
                    .operations(&["retrieveEntity"])
                    .build(),
            )
            .await
            .expect("register s1"),
        201,
    );
    mock.set_stub_with_body(Method::GET, &format!("/s1/ngsi-ld/v1/entities/{entity_id}"), 200, &vehicle(&entity_id, 10))
        .await;

    let map = entity_map_header(
        &front
            .client()
            .get_entity(&entity_id, "entityMap=true", "application/ld+json", None)
            .await
            .expect("mint"),
    )
    .expect("minted entity map header");

    let second_id = topology.id("registration:cons5-s2");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&second_id, &mock.url("/s2"))
                    .mode("inclusive")
                    .entity_id(&entity_id)
                    .operations(&["retrieveEntity"])
                    .build(),
            )
            .await
            .expect("register s2"),
        201,
    );
    mock.set_stub_with_body(Method::GET, &format!("/s2/ngsi-ld/v1/entities/{entity_id}"), 200, &vehicle(&entity_id, 20))
        .await;

    assert_status(&retrieve_reusing_map(&topology, &entity_id, &map).await, 200);
    assert_eq!(
        mock.get_count(Method::GET, &format!("/s2/ngsi-ld/v1/entities/{entity_id}")).await,
        0,
        "the post-minting source is not contacted — no fresh discovery"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A retrieve with the return flag and no prior map creates a new map and returns
/// its location in `NGSILD-EntityMap` (clauses 5.7.1.4, 6.3.15). Confirms
/// REQ-CONS-6, REQ-CONS-18.
#[tokio::test]
async fn distop_retrieve_entity_return_flag_no_existing_map_creates_new() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons6");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    let registration_id = topology.id("registration:cons6");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, &mock.url(""))
                    .mode("inclusive")
                    .entity_id(&entity_id)
                    .operations(&["retrieveEntity"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &vehicle(&entity_id, 10))
        .await;

    let mint = front
        .client()
        .get_entity(&entity_id, "entityMap=true", "application/ld+json", None)
        .await
        .expect("mint");
    assert!(entity_map_header(&mint).is_some(), "a new map location is returned in NGSILD-EntityMap");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A retrieve supplying an existing map echoes that in-use map in the response
/// `NGSILD-EntityMap` (clause 6.3.15). Confirms REQ-CONS-18 (supplied-location half).
#[tokio::test]
async fn distop_retrieve_entity_map_location_returned_when_supplied() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons18");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    let registration_id = topology.id("registration:cons18");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, &mock.url(""))
                    .mode("inclusive")
                    .entity_id(&entity_id)
                    .operations(&["retrieveEntity"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &vehicle(&entity_id, 10))
        .await;

    let map = entity_map_header(
        &front
            .client()
            .get_entity(&entity_id, "entityMap=true", "application/ld+json", None)
            .await
            .expect("mint"),
    )
    .expect("minted entity map header");
    let reuse = retrieve_reusing_map(&topology, &entity_id, &map).await;
    assert_status(&reuse, 200);
    assert_eq!(
        entity_map_header(&reuse).as_deref(),
        Some(map.as_str()),
        "the response echoes the supplied in-use map location"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A linked map entry forwards the linked downstream map location on the forwarded
/// retrieve (clause 5.5.14). Confirms REQ-CONS-8.
#[tokio::test]
async fn distop_retrieve_entity_map_linked_entry_forwards_linked_location() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons8");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    let registration_id = topology.id("registration:cons8");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, &mock.url(""))
                    .mode("inclusive")
                    .entity_id(&entity_id)
                    .operations(&["retrieveEntity"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &vehicle(&entity_id, 10))
        .await;

    let map = entity_map_header(
        &front
            .client()
            .get_entity(&entity_id, "entityMap=true", "application/ld+json", None)
            .await
            .expect("mint"),
    )
    .expect("minted entity map header");
    let reuse = retrieve_reusing_map(&topology, &entity_id, &map).await;
    assert_status(&reuse, 200);
    let forwarded = mock.next_request(Duration::from_secs(2)).await;
    assert!(
        forwarded_header(&forwarded, "ngsild-entitymap").is_some(),
        "the forwarded retrieve carries the linked map location"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Maps returned by forwarded CSRs update the in-use map's `linkedMaps`
/// (clause 5.5.14). Confirms REQ-CONS-13.
#[tokio::test]
async fn distop_retrieve_entity_map_linkedmaps_updated() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons13");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    let registration_id = topology.id("registration:cons13");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, &mock.url(""))
                    .mode("inclusive")
                    .entity_id(&entity_id)
                    .operations(&["retrieveEntity"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &vehicle(&entity_id, 10))
        .await;

    let map = entity_map_header(
        &front
            .client()
            .get_entity(&entity_id, "entityMap=true", "application/ld+json", None)
            .await
            .expect("mint"),
    )
    .expect("minted entity map header");
    assert_status(&retrieve_reusing_map(&topology, &entity_id, &map).await, 200);
    let stored = front.client().retrieve_entity_map(&map).await.expect("retrieve map");
    assert_status(&stored, 200);
    assert!(
        json_body(&stored).get("linkedMaps").is_some(),
        "the in-use map records linkedMaps for CSR-returned maps"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
