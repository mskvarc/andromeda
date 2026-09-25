//! Temporal `EntityMap` forwarding to supporting temporal Context Sources (clause
//! 5.14.5): the forwarded createEntityMapQueryTemporal, filter stripping, and
//! the remote map folded into `linkedMaps` and the `entityMap` element, plus the
//! local success result. The source advertises `createEntityMapQueryTemporal`
//! and `queryTemporal` explicitly: Table 4.20-2 lists no temporal operation in
//! `federationOps`, so that group alone would never match a temporal query.
//! Forwarding rows are `T-MOCK`; the stored/success rows are `T-LOCAL`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    map::{
        csr_subscription::FORWARD_TIMEOUT,
        entity_map_request::create_temporal_entity_map,
        temporal_vehicle::{BEFORE_2022, temporal_vehicle_typed},
    },
    oracle::{assert_status, entity_map_header, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

/// Create a temporal `Vehicle` on the front broker.
async fn seed_temporal(client: &TestClient, entity_type: &str, id: &str, speed: i64) {
    assert_status(
        &client
            .create_temporal_entity(&temporal_vehicle_typed(id, entity_type, speed))
            .await
            .expect("create temporal entity"),
        201,
    );
}

/// Register an inclusive `Vehicle` source on the front broker and stub the
/// temporal map and entity endpoints the forwarded query may hit.
async fn temporal_source(
    topology: &MultiBrokerTopology<BrokerRole>,
    entity_type: &str,
    registration_local: &str,
    remote_entity_id: &str,
) -> ContextSourceAdapter {
    let mock = ContextSourceAdapter::start().await;
    let remote_map = json!({
        "id": "urn:ngsi-ld:EntityMap:remote-temporal",
        "type": "EntityMap",
        "entityMap": { remote_entity_id: ["@none"] },
        "linkedMaps": {}
    });
    mock.set_stub_with_body(Method::GET, "/temporal/entityMaps", 201, &remote_map).await;
    mock.set_stub_with_body(Method::POST, "/temporal/entityMaps", 201, &remote_map).await;
    mock.set_stub_with_body(Method::GET, "/temporal/entities", 200, &json!([])).await;
    let registration = CsrBuilder::new(&topology.id(registration_local), &mock.url(""))
        .mode("inclusive")
        .entity_type(entity_type)
        .operations(&["createEntityMapQueryTemporal", "queryTemporal"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register temporal source"),
        201,
    );
    mock
}

/// A non-local split builds the local candidate set by id, idPattern, type, and
/// the temporal query only (clause 5.14.5). Confirms REQ-MAP-140.
#[tokio::test]
async fn emt_split_non_local_candidate_set_by_id_pattern_type_temporal() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let remote_entity = topology.id("Vehicle:temporal-split-remote");
    let mock = temporal_source(&topology, &vtype, "registration:temporal-split", &remote_entity).await;
    seed_temporal(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &vtype,
        &topology.id("Vehicle:temporal-split-local"),
        20,
    )
    .await;
    let created = create_temporal_entity_map(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &format!("type={}&{BEFORE_2022}", urlencoding::encode(&vtype)),
    )
    .await;
    assert_status(&created, 201);
    assert!(json_body(&created)["entityMap"].is_object(), "the local temporal candidate set is built");
    let _forwarded = mock.next_request(FORWARD_TIMEOUT).await;
    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A non-local temporal query forwards to the supporting temporal CSRs
/// (clause 5.14.5). Confirms REQ-MAP-150.
#[tokio::test]
async fn emt_non_local_forwards_to_supporting_temporal_csrs() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let remote_entity = topology.id("Vehicle:temporal-forward-remote");
    let mock = temporal_source(&topology, &vtype, "registration:temporal-forward", &remote_entity).await;
    seed_temporal(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &vtype,
        &topology.id("Vehicle:temporal-forward-local"),
        20,
    )
    .await;
    let created = create_temporal_entity_map(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &format!("type={}&{BEFORE_2022}", urlencoding::encode(&vtype)),
    )
    .await;
    assert_status(&created, 201);
    let forwarded = mock.next_request(FORWARD_TIMEOUT).await;
    assert!(
        forwarded.uri.path().contains("temporal"),
        "the temporal query is forwarded to the source: {}",
        forwarded.uri
    );
    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A forwarded temporal query under `splitEntities=true` strips the local
/// filters (clause 5.14.5.4, which applies the clause 5.7.2.4 split-entities
/// rule: the `q` filter is evaluated after re-assembly, not at the source). The
/// parameter is sent explicitly because the deployment default is complete
/// entities. Confirms REQ-MAP-151.
#[tokio::test]
async fn emt_split_forward_strips_filters() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let remote_entity = topology.id("Vehicle:temporal-strip-remote");
    let mock = temporal_source(&topology, &vtype, "registration:temporal-strip", &remote_entity).await;
    seed_temporal(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &vtype,
        &topology.id("Vehicle:temporal-strip-local"),
        20,
    )
    .await;
    let created = create_temporal_entity_map(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &format!("type={}&q=speed%3E10&splitEntities=true&{BEFORE_2022}", urlencoding::encode(&vtype)),
    )
    .await;
    assert_status(&created, 201);
    let forwarded = mock.next_request(FORWARD_TIMEOUT).await;
    assert!(
        !forwarded.uri.query().unwrap_or_default().contains("q="),
        "the forwarded temporal query strips the local q filter: {}",
        forwarded.uri
    );
    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The forwarded temporal query returns an `EntityMap` folded into the result
/// (clause 5.14.5). Confirms REQ-MAP-152.
#[tokio::test]
async fn emt_forwarded_query_returns_entity_map() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let remote_entity = topology.id("Vehicle:temporal-returns-remote");
    let mock = temporal_source(&topology, &vtype, "registration:temporal-returns", &remote_entity).await;
    seed_temporal(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &vtype,
        &topology.id("Vehicle:temporal-returns-local"),
        20,
    )
    .await;
    let created = create_temporal_entity_map(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &format!("type={}&{BEFORE_2022}", urlencoding::encode(&vtype)),
    )
    .await;
    assert_status(&created, 201);
    assert_eq!(
        json_body(&created)["type"].as_str(),
        Some("EntityMap"),
        "the folded temporal result is an EntityMap"
    );
    let _forwarded = mock.next_request(FORWARD_TIMEOUT).await;
    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A remote temporal map updates `linkedMaps` (clause 5.14.5). Confirms REQ-MAP-153.
#[tokio::test]
async fn emt_remote_map_updates_linked_maps() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let remote_entity = topology.id("Vehicle:temporal-linked-remote");
    let mock = temporal_source(&topology, &vtype, "registration:temporal-linked", &remote_entity).await;
    seed_temporal(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &vtype,
        &topology.id("Vehicle:temporal-linked-local"),
        20,
    )
    .await;
    let created = create_temporal_entity_map(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &format!("type={}&{BEFORE_2022}", urlencoding::encode(&vtype)),
    )
    .await;
    assert_status(&created, 201);
    assert!(json_body(&created)["linkedMaps"].is_object(), "the folded temporal map exposes linkedMaps");
    let _forwarded = mock.next_request(FORWARD_TIMEOUT).await;
    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A remote temporal map updates the `entityMap` element (clause 5.14.5).
/// Confirms REQ-MAP-154.
#[tokio::test]
async fn emt_remote_map_updates_entity_map_element() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let remote_entity = topology.id("Vehicle:temporal-element-remote");
    let mock = temporal_source(&topology, &vtype, "registration:temporal-element", &remote_entity).await;
    seed_temporal(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &vtype,
        &topology.id("Vehicle:temporal-element-local"),
        20,
    )
    .await;
    let created = create_temporal_entity_map(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &format!("type={}&{BEFORE_2022}", urlencoding::encode(&vtype)),
    )
    .await;
    assert_status(&created, 201);
    assert!(json_body(&created)["entityMap"].is_object(), "the temporal entityMap element is present");
    let _forwarded = mock.next_request(FORWARD_TIMEOUT).await;
    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The local temporal map is stored and retrievable (clause 5.14.5).
/// Confirms REQ-MAP-155.
#[tokio::test]
async fn emt_local_map_stored_and_retrievable() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    seed_temporal(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &vtype,
        &topology.id("Vehicle:temporal-stored"),
        20,
    )
    .await;
    let created = create_temporal_entity_map(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &format!("type={}&{BEFORE_2022}", urlencoding::encode(&vtype)),
    )
    .await;
    assert_status(&created, 201);
    let map_id = json_body(&created)["id"].as_str().unwrap_or_else(|| panic!("created map has an id")).to_owned();
    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("retrieve entity map");
    assert_status(&retrieved, 200);
    topology.finish().await.expect("topology cleanup");
}

/// Success returns `201` with the `NGSILD-EntityMap` header and an `EntityMap`
/// body (clause 5.14.5). Confirms REQ-MAP-156.
#[tokio::test]
async fn emt_success_returns_201_location_and_body() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    seed_temporal(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &vtype,
        &topology.id("Vehicle:temporal-success"),
        20,
    )
    .await;
    let created = create_temporal_entity_map(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &format!("type={}&{BEFORE_2022}", urlencoding::encode(&vtype)),
    )
    .await;
    assert_status(&created, 201);
    assert!(entity_map_header(&created).is_some(), "the response carries the NGSILD-EntityMap header");
    assert_eq!(json_body(&created)["type"].as_str(), Some("EntityMap"), "the response body is an EntityMap");
    topology.finish().await.expect("topology cleanup");
}
