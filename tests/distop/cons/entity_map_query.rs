//! Query Entities entity-map create and reuse (clauses 5.7.2.4, 5.5.14, 6.3.15):
//! minting a map and returning `201`, reusing a valid map so its CSR set is fixed,
//! and recreating on an expired/missing map. The linked cross-instance map row
//! (`REQ-CONS-26`) runs against the durable, shared entity-map store.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    oracle::{assert_status, entity_map_header, forwarded_header},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_client::response::TestResponse;
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use axum::http::Method;
use serde_json::json;
use std::time::Duration;

/// Query the front broker reusing a supplied entity map.
async fn query_reusing_map(topology: &MultiBrokerTopology<BrokerRole>, query: &str, map: &str) -> TestResponse {
    topology
        .broker(BrokerRole::A)
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/entities?{query}"))
        .header("accept", "application/ld+json")
        .header("NGSILD-EntityMap", map)
        .send()
        .await
        .expect("reuse query")
}

/// A minted query map fixes the CSR set on reuse and is recreated on a bogus map:
/// a source registered after minting is consulted only after recreation
/// (clause 5.5.14). Reusing the live map answers `200`; the bogus-map leg mints
/// a replacement and answers `201` (Table 6.4.3.2-3: "201 Created (in case an
/// `EntityMap` has been (re)created)"). Confirms REQ-CONS-23.
#[tokio::test]
async fn distop_query_entities_map_location_reused_or_recreated() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    let first_id = topology.id("registration:cons23-s1");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&first_id, &mock.url("/s1"))
                    .mode("inclusive")
                    .operations(&["queryEntity"])
                    .build(),
            )
            .await
            .expect("register s1"),
        201,
    );
    mock.set_stub_with_body(
        Method::GET,
        "/s1/ngsi-ld/v1/entities",
        200,
        &json!([vehicle(&topology.id("Vehicle:cons23"), 10)]),
    )
    .await;

    let mint = front
        .client()
        .query_entities("type=Vehicle&entityMap=true", "application/ld+json", None)
        .await
        .expect("mint");
    assert_status(&mint, 201);
    let map = entity_map_header(&mint).expect("minted entity map header");

    let second_id = topology.id("registration:cons23-s2");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&second_id, &mock.url("/s2"))
                    .mode("inclusive")
                    .operations(&["queryEntity"])
                    .build(),
            )
            .await
            .expect("register s2"),
        201,
    );
    mock.set_stub_with_body(Method::GET, "/s2/ngsi-ld/v1/entities", 200, &json!([])).await;

    assert_status(&query_reusing_map(&topology, "type=Vehicle", &map).await, 200);
    assert_eq!(
        mock.get_count(Method::GET, "/s2/ngsi-ld/v1/entities"),
        0,
        "a valid map fixes the CSR set and the new source is not consulted"
    );

    let bogus = topology.id("EntityMap:bogus");
    assert_status(&query_reusing_map(&topology, "type=Vehicle", &bogus).await, 201);
    assert!(
        mock.get_count(Method::GET, "/s2/ngsi-ld/v1/entities") > 0,
        "a bogus map is recreated by fresh discovery, now consulting the new source"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A query with the return flag and no existing map creates a new map and returns
/// `201` with the `NGSILD-EntityMap` header (clause 6.3.15). Confirms REQ-CONS-24.
#[tokio::test]
async fn distop_query_entities_return_flag_creates_map_and_header() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A);
    let entity_id = topology.id("Vehicle:cons24");
    assert_status(
        &front.client().post_entity(&vehicle(&entity_id, 10)).with_ld_json().send().await.expect("local"),
        201,
    );

    let response = front
        .client()
        .query_entities("type=Vehicle&entityMap=true", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 201);
    assert!(entity_map_header(&response).is_some(), "a new map location is returned in NGSILD-EntityMap");

    topology.finish().await.expect("topology cleanup");
}

/// A query reusing a map whose CSR entry links a downstream map forwards that
/// linked location (clause 5.5.14). Confirms REQ-CONS-26.
#[tokio::test]
async fn distop_query_entities_map_linked_entry_forwards_linked_location() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    let registration_id = topology.id("registration:cons26");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, &mock.url(""))
                    .mode("inclusive")
                    .operations(&["queryEntity"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &json!([vehicle(&topology.id("Vehicle:cons26"), 10)]))
        .await;

    let map = entity_map_header(
        &front
            .client()
            .query_entities("type=Vehicle&entityMap=true", "application/ld+json", None)
            .await
            .expect("mint"),
    )
    .expect("minted entity map header");
    assert_status(&query_reusing_map(&topology, "type=Vehicle", &map).await, 200);
    let forwarded = mock.next_request(Duration::from_secs(2)).await;
    assert!(
        forwarded_header(&forwarded, "ngsild-entitymap").is_some(),
        "the forwarded query carries the linked map location"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
