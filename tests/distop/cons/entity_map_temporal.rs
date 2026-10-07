//! Temporal entity-map create and reuse for Retrieve/Query Temporal (clauses 5.7.3.4, 5.7.4.4, 5.5.14, 6.3.15): minting, fixing the CSR set on reuse, recreating on an expired/missing map, and returning the map location.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::entity,
    oracle::{assert_status, entity_map_header, forwarded_header, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_client::response::TestResponse;
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::time::Duration;
use urlencoding::encode;

/// A single temporal `Property` instance at the given `observedAt`.
fn observed(value: i64, observed_at: &str) -> Value {
    json!({ "type": "Property", "value": value, "observedAt": observed_at })
}

const BEFORE_2030: &str = "timerel=before&timeAt=2030-01-01T00:00:00Z";

/// Retrieve one temporal entity reusing a supplied map.
async fn retrieve_temporal_reusing_map(topology: &MultiBrokerTopology<BrokerRole>, entity_id: &str, map: &str) -> TestResponse {
    topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{}?{BEFORE_2030}", encode(entity_id)))
        .header("accept", "application/ld+json")
        .header("NGSILD-EntityMap", map)
        .send()
        .await
        .expect("reuse temporal retrieve")
}

/// Query temporal entities of a per-test type reusing a supplied map.
async fn query_temporal_reusing_map(topology: &MultiBrokerTopology<BrokerRole>, vtype: &str, map: &str) -> TestResponse {
    topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/temporal/entities?type={}&{BEFORE_2030}", encode(vtype)))
        .header("accept", "application/ld+json")
        .header("NGSILD-EntityMap", map)
        .send()
        .await
        .expect("reuse temporal query")
}

/// A temporal retrieve map fixes the CSR set on reuse and is recreated on a bogus map (clause 5.5.14). Reusing the live map answers `200`; the bogus-map leg mints a replacement and answers `201` (Table 6.4.3.2-3 applies the same "201 Created (in case an `EntityMap` has been (re)created)" rule to every map-bearing read).
#[tokio::test]
async fn distop_retrieve_temporal_map_reused_or_recreated() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons42");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    let first_id = topology.id("registration:cons42-s1");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&first_id, &mock.url("/s1"))
                    .mode("inclusive")
                    .entity_id(&entity_id)
                    .operations(&["retrieveTemporal"])
                    .build(),
            )
            .await
            .expect("register s1"),
        201,
    );
    mock.set_stub_with_body(
        Method::GET,
        &format!("/s1/ngsi-ld/v1/temporal/entities/{entity_id}"),
        200,
        &entity(&entity_id, "Vehicle", &[("speed", json!([observed(50, "2020-08-01T12:00:00Z")]))]),
    )
    .await;

    let map = entity_map_header(
        &front
            .client()
            .get_temporal_entity(&entity_id, &format!("entityMap=true&{BEFORE_2030}"), None)
            .await
            .expect("mint"),
    )
    .expect("minted entity map header");

    let second_id = topology.id("registration:cons42-s2");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&second_id, &mock.url("/s2"))
                    .mode("inclusive")
                    .entity_id(&entity_id)
                    .operations(&["retrieveTemporal"])
                    .build(),
            )
            .await
            .expect("register s2"),
        201,
    );
    mock.set_stub_with_body(
        Method::GET,
        &format!("/s2/ngsi-ld/v1/temporal/entities/{entity_id}"),
        200,
        &entity(&entity_id, "Vehicle", &[("speed", json!([observed(60, "2020-08-01T12:00:00Z")]))]),
    )
    .await;

    assert_status(&retrieve_temporal_reusing_map(&topology, &entity_id, &map).await, 200);
    assert_eq!(
        mock.get_count(Method::GET, &format!("/s2/ngsi-ld/v1/temporal/entities/{entity_id}")),
        0,
        "a valid map fixes the CSR set"
    );

    let bogus = topology.id("EntityMap:bogus");
    assert_status(&retrieve_temporal_reusing_map(&topology, &entity_id, &bogus).await, 201);
    assert!(
        mock.get_count(Method::GET, &format!("/s2/ngsi-ld/v1/temporal/entities/{entity_id}")) > 0,
        "a bogus map is recreated by fresh discovery"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A temporal retrieve with the return flag and no existing map creates a new map and returns its location (clause 6.3.15).
#[tokio::test]
async fn distop_retrieve_temporal_return_flag_creates_new_map() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons43");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    let registration_id = topology.id("registration:cons43");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, &mock.url(""))
                    .mode("inclusive")
                    .entity_id(&entity_id)
                    .operations(&["retrieveTemporal"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );
    mock.set_stub_with_body(
        Method::GET,
        &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"),
        200,
        &entity(&entity_id, "Vehicle", &[("speed", json!([observed(50, "2020-08-01T12:00:00Z")]))]),
    )
    .await;

    let mint = front
        .client()
        .get_temporal_entity(&entity_id, &format!("entityMap=true&{BEFORE_2030}"), None)
        .await
        .expect("mint");
    assert!(entity_map_header(&mint).is_some(), "a new temporal map location is returned");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A temporal retrieve reusing a map whose CSR entry links a downstream map forwards that linked location (clause 5.5.14).
#[tokio::test]
async fn distop_retrieve_temporal_map_linked_entry_forwards_linked_location() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons45");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    let registration_id = topology.id("registration:cons45");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, &mock.url(""))
                    .mode("inclusive")
                    .entity_id(&entity_id)
                    .operations(&["retrieveTemporal"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );
    mock.set_stub_with_body(
        Method::GET,
        &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"),
        200,
        &entity(&entity_id, "Vehicle", &[("speed", json!([observed(50, "2020-08-01T12:00:00Z")]))]),
    )
    .await;

    let map = entity_map_header(
        &front
            .client()
            .get_temporal_entity(&entity_id, &format!("entityMap=true&{BEFORE_2030}"), None)
            .await
            .expect("mint"),
    )
    .expect("minted entity map header");
    assert_status(&retrieve_temporal_reusing_map(&topology, &entity_id, &map).await, 200);
    let forwarded = mock.next_request(Duration::from_secs(2)).await;
    assert!(
        forwarded_header(&forwarded, "ngsild-entitymap").is_some(),
        "the forwarded temporal retrieve carries the linked map location"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Maps returned by forwarded CSRs update a temporal map's `linkedMaps` (clause 5.5.14).
#[tokio::test]
async fn distop_retrieve_temporal_map_linkedmaps_updated() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons48");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    let registration_id = topology.id("registration:cons48");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, &mock.url(""))
                    .mode("inclusive")
                    .entity_id(&entity_id)
                    .operations(&["retrieveTemporal"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );
    mock.set_stub_with_body(
        Method::GET,
        &format!("/ngsi-ld/v1/temporal/entities/{entity_id}"),
        200,
        &entity(&entity_id, "Vehicle", &[("speed", json!([observed(50, "2020-08-01T12:00:00Z")]))]),
    )
    .await;

    let map = entity_map_header(
        &front
            .client()
            .get_temporal_entity(&entity_id, &format!("entityMap=true&{BEFORE_2030}"), None)
            .await
            .expect("mint"),
    )
    .expect("minted entity map header");
    assert_status(&retrieve_temporal_reusing_map(&topology, &entity_id, &map).await, 200);
    let stored = front.client().retrieve_entity_map(&map).await.expect("retrieve map");
    assert_status(&stored, 200);
    assert!(
        json_body(&stored).get("linkedMaps").is_some(),
        "the temporal map records linkedMaps for CSR-returned maps"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A temporal query map fixes the CSR set on reuse and is recreated on a bogus map (clause 5.5.14). Reusing the live map answers `200`; the bogus-map leg mints a replacement and answers `201` (Table 6.4.3.2-3: "201 Created (in case an `EntityMap` has been (re)created)").
#[tokio::test]
async fn distop_query_temporal_map_reused_or_recreated() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    let vtype = topology.id("Vehicle");
    let first_id = topology.id("registration:cons58-s1");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&first_id, &mock.url("/s1"))
                    .mode("inclusive")
                    .entity_type(&vtype)
                    .operations(&["queryTemporal"])
                    .build(),
            )
            .await
            .expect("register s1"),
        201,
    );
    mock.set_stub_with_body(
        Method::GET,
        "/s1/ngsi-ld/v1/temporal/entities",
        200,
        &json!([entity(
            &topology.id("Vehicle:cons58"),
            &vtype,
            &[("speed", json!([observed(50, "2020-08-01T12:00:00Z")]))]
        )]),
    )
    .await;

    let map = entity_map_header(
        &front
            .client()
            .query_temporal_entities(&format!("type={}&{BEFORE_2030}&entityMap=true", encode(&vtype)), None)
            .await
            .expect("mint"),
    )
    .expect("minted entity map header");

    let second_id = topology.id("registration:cons58-s2");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&second_id, &mock.url("/s2"))
                    .mode("inclusive")
                    .entity_type(&vtype)
                    .operations(&["queryTemporal"])
                    .build(),
            )
            .await
            .expect("register s2"),
        201,
    );
    mock.set_stub_with_body(Method::GET, "/s2/ngsi-ld/v1/temporal/entities", 200, &json!([])).await;

    assert_status(&query_temporal_reusing_map(&topology, &vtype, &map).await, 200);
    assert_eq!(
        mock.get_count(Method::GET, "/s2/ngsi-ld/v1/temporal/entities"),
        0,
        "a valid temporal map fixes the CSR set"
    );

    let bogus = topology.id("EntityMap:bogus");
    assert_status(&query_temporal_reusing_map(&topology, &vtype, &bogus).await, 201);
    assert!(
        mock.get_count(Method::GET, "/s2/ngsi-ld/v1/temporal/entities") > 0,
        "a bogus temporal map is recreated by fresh discovery"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A temporal query with the return flag and no existing map creates a new map and returns its location header (clause 6.3.15).
#[tokio::test]
async fn distop_query_temporal_return_flag_creates_new_map() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:cons59");
    let vtype = topology.id("Vehicle");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    assert_status(
        &front
            .client()
            .create_temporal_entity(&entity(&entity_id, &vtype, &[("speed", json!([observed(50, "2020-08-01T12:00:00Z")]))]))
            .await
            .expect("local"),
        201,
    );

    let mint = front
        .client()
        .query_temporal_entities(&format!("type={}&{BEFORE_2030}&entityMap=true", encode(&vtype)), None)
        .await
        .expect("mint");
    assert_status(&mint, 201);
    assert!(entity_map_header(&mint).is_some(), "a new temporal query map location is returned");

    topology.finish().await.expect("topology cleanup");
}

/// A temporal query reusing a map whose CSR entry links a downstream map forwards that linked location (clause 5.5.14).
#[tokio::test]
async fn distop_query_temporal_map_linked_entry_forwards_linked_location() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;

    let vtype = topology.id("Vehicle");
    let registration_id = topology.id("registration:cons61");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, &mock.url(""))
                    .mode("inclusive")
                    .entity_type(&vtype)
                    .operations(&["queryTemporal"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );
    mock.set_stub_with_body(
        Method::GET,
        "/ngsi-ld/v1/temporal/entities",
        200,
        &json!([entity(
            &topology.id("Vehicle:cons61"),
            &vtype,
            &[("speed", json!([observed(50, "2020-08-01T12:00:00Z")]))]
        )]),
    )
    .await;

    let map = entity_map_header(
        &front
            .client()
            .query_temporal_entities(&format!("type={}&{BEFORE_2030}&entityMap=true", encode(&vtype)), None)
            .await
            .expect("mint"),
    )
    .expect("minted entity map header");
    assert_status(&query_temporal_reusing_map(&topology, &vtype, &map).await, 200);
    let forwarded = mock.next_request(Duration::from_secs(2)).await;
    assert!(
        forwarded_header(&forwarded, "ngsild-entitymap").is_some(),
        "the forwarded temporal query carries the linked map location"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
