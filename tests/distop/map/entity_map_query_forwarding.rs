//! EntityMap-from-query map building and forwarding to supporting Context
//! Sources (clause 5.14.4): the fully-filtered local map, and the forwarded
//! createEntityMapQueryEntity that folds a remote map into `linkedMaps` and the
//! `entityMap` element. Local rows are `T-LOCAL`; forwarding rows are `T-MOCK`,
//! seeded `UNKNOWN`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property, vehicle},
    map::{csr_subscription::FORWARD_TIMEOUT, entity_map_request::create_entity_map},
    oracle::{assert_status, entity_map_header, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use axum::http::Method;
use serde_json::{Value, json};

/// Create a `Vehicle` with the given speed on the front broker.
async fn seed_vehicle(client: &TestClient, id: &str, speed: i64) {
    assert_status(
        &client.post_entity(&vehicle(id, speed)).with_ld_json().send().await.expect("create vehicle"),
        201,
    );
}

/// Register an inclusive `Vehicle` source on the front broker and stub the map
/// and entity endpoints the forwarded query may hit.
async fn query_source(topology: &MultiBrokerTopology<BrokerRole>, registration_local: &str, remote_entity_id: &str) -> ContextSourceAdapter {
    let mock = ContextSourceAdapter::start().await;
    let remote_map = json!({
        "id": "urn:ngsi-ld:EntityMap:remote",
        "type": "EntityMap",
        "entityMap": { remote_entity_id: ["@none"] },
        "linkedMaps": {}
    });
    mock.set_stub_with_body(Method::GET, "/entityMaps", 201, &remote_map).await;
    mock.set_stub_with_body(Method::POST, "/entityMaps", 201, &remote_map).await;
    mock.set_stub_with_body(Method::GET, "/entities", 200, &json!([])).await;
    let registration = CsrBuilder::new(&topology.id(registration_local), &mock.url(""))
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["federationOps"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&registration)
            .await
            .expect("register query source"),
        201,
    );
    mock
}

/// A non-local split builds the local map by id, type, and idPattern only
/// (clause 5.14.4). Confirms REQ-MAP-121.
#[tokio::test]
async fn emq_split_non_local_builds_local_map_by_id_type_pattern() {
    let topology = topology::single().await.expect("single-broker topology");
    let remote_entity = topology.id("Vehicle:split-remote");
    let mock = query_source(&topology, "registration:split-local-map", &remote_entity).await;
    seed_vehicle(topology.broker(BrokerRole::A).client(), &topology.id("Vehicle:split-local"), 20).await;
    let created = create_entity_map(topology.broker(BrokerRole::A).client(), "type=Vehicle&q=speed%3E10").await;
    assert_status(&created, 201);
    assert!(json_body(&created)["entityMap"].is_object(), "the local map is built for the split query");
    let _forwarded = mock.next_request(FORWARD_TIMEOUT).await;
    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A non-split local query builds a fully filtered map (clause 5.14.4).
/// Confirms REQ-MAP-122.
#[tokio::test]
async fn emq_non_split_local_builds_fully_filtered_map() {
    let topology = topology::single().await.expect("single-broker topology");
    let fast = topology.id("Vehicle:filtered-fast");
    let slow = topology.id("Vehicle:filtered-slow");
    seed_vehicle(topology.broker(BrokerRole::A).client(), &fast, 100).await;
    seed_vehicle(topology.broker(BrokerRole::A).client(), &slow, 20).await;
    let created = create_entity_map(topology.broker(BrokerRole::A).client(), "type=Vehicle&q=speed%3E50").await;
    assert_status(&created, 201);
    let map = json_body(&created)["entityMap"].clone();
    assert!(map.get(&fast).is_some(), "the matching entity is in the map");
    assert!(map.get(&slow).is_none(), "the non-matching entity is filtered out");
    topology.finish().await.expect("topology cleanup");
}

/// Any matching instance of a multi-instance geoproperty keeps the entity in the
/// map (clause 5.14.4). Confirms REQ-MAP-123.
#[tokio::test]
async fn emq_multi_instance_geoproperty_any_match_keeps_entity() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:geo-any-match");
    let body = json!({
        "id": entity_id,
        "type": "Vehicle",
        "location": [
            { "type": "GeoProperty", "value": { "type": "Point", "coordinates": [0, 0] }, "datasetId": "urn:ngsi-ld:ds:here" },
            { "type": "GeoProperty", "value": { "type": "Point", "coordinates": [80, 80] }, "datasetId": "urn:ngsi-ld:ds:far" }
        ],
        "@context": COMPOUND_CONTEXT.as_str()
    });
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&body)
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let created = create_entity_map(
        topology.broker(BrokerRole::A).client(),
        "type=Vehicle&georel=near%3BmaxDistance%3D%3D2000&geometry=Point&coordinates=%5B0%2C0%5D",
    )
    .await;
    assert_status(&created, 201);
    assert!(
        json_body(&created)["entityMap"].get(&entity_id).is_some(),
        "one matching geoproperty instance keeps the entity"
    );
    topology.finish().await.expect("topology cleanup");
}

/// An attribute list keeps only the entities that hold the attribute
/// (clause 5.14.4). Confirms REQ-MAP-124.
#[tokio::test]
async fn emq_attribute_list_keeps_only_holders() {
    let topology = topology::single().await.expect("single-broker topology");
    let holder = topology.id("Vehicle:attr-holder");
    seed_vehicle(topology.broker(BrokerRole::A).client(), &holder, 20).await;
    let other = topology.id("Vehicle:attr-non-holder");
    let non_holder = entity(&other, "Vehicle", &[("colour", property(json!("red")))]);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&non_holder)
            .with_ld_json()
            .send()
            .await
            .expect("create non-holder"),
        201,
    );
    let created = create_entity_map(topology.broker(BrokerRole::A).client(), "type=Vehicle&attrs=speed").await;
    assert_status(&created, 201);
    let map = json_body(&created)["entityMap"].clone();
    assert!(map.get(&holder).is_some(), "the attribute holder is kept");
    assert!(map.get(&other).is_none(), "the non-holder is dropped");
    topology.finish().await.expect("topology cleanup");
}

/// A non-local query forwards createEntityMapQueryEntity to the supporting CSR
/// (clause 5.14.4). Confirms REQ-MAP-125.
#[tokio::test]
async fn emq_non_local_forwards_to_supporting_csrs() {
    let topology = topology::single().await.expect("single-broker topology");
    let remote_entity = topology.id("Vehicle:forward-remote");
    let mock = query_source(&topology, "registration:forward-supporting", &remote_entity).await;
    seed_vehicle(topology.broker(BrokerRole::A).client(), &topology.id("Vehicle:forward-local"), 20).await;
    let created = create_entity_map(topology.broker(BrokerRole::A).client(), "type=Vehicle").await;
    assert_status(&created, 201);
    let forwarded = mock.next_request(FORWARD_TIMEOUT).await;
    assert!(
        forwarded.uri.path().contains("entit"),
        "the query is forwarded to the source: {}",
        forwarded.uri
    );
    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A forwarded query under `splitEntities=true` strips the local filters
/// (clause 5.14.4.4 applies the clause 5.7.2.4 split-entities rule: the `q`
/// filter is evaluated after re-assembly, not at the source). The parameter is
/// sent explicitly because the deployment default is complete entities.
/// Confirms REQ-MAP-126.
#[tokio::test]
async fn emq_split_forward_strips_filters() {
    let topology = topology::single().await.expect("single-broker topology");
    let remote_entity = topology.id("Vehicle:strip-remote");
    let mock = query_source(&topology, "registration:strip-filters", &remote_entity).await;
    seed_vehicle(topology.broker(BrokerRole::A).client(), &topology.id("Vehicle:strip-local"), 20).await;
    let created = create_entity_map(topology.broker(BrokerRole::A).client(), "type=Vehicle&q=speed%3E10&splitEntities=true").await;
    assert_status(&created, 201);
    let forwarded = mock.next_request(FORWARD_TIMEOUT).await;
    assert!(
        !forwarded.uri.query().unwrap_or_default().contains("q="),
        "the forwarded query strips the local q filter: {}",
        forwarded.uri
    );
    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The forwarded query returns an EntityMap that the front broker folds in
/// (clause 5.14.4). Confirms REQ-MAP-127.
#[tokio::test]
async fn emq_forwarded_query_returns_entity_map() {
    let topology = topology::single().await.expect("single-broker topology");
    let remote_entity = topology.id("Vehicle:returns-remote");
    let mock = query_source(&topology, "registration:returns-map", &remote_entity).await;
    seed_vehicle(topology.broker(BrokerRole::A).client(), &topology.id("Vehicle:returns-local"), 20).await;
    let created = create_entity_map(topology.broker(BrokerRole::A).client(), "type=Vehicle").await;
    assert_status(&created, 201);
    assert_eq!(json_body(&created)["type"].as_str(), Some("EntityMap"), "the folded result is an EntityMap");
    let _forwarded = mock.next_request(FORWARD_TIMEOUT).await;
    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A remote map updates `linkedMaps` with its id (clause 5.14.4).
/// Confirms REQ-MAP-128.
#[tokio::test]
async fn emq_remote_map_updates_linked_maps() {
    let topology = topology::single().await.expect("single-broker topology");
    let remote_entity = topology.id("Vehicle:linked-remote");
    let mock = query_source(&topology, "registration:linked-maps", &remote_entity).await;
    seed_vehicle(topology.broker(BrokerRole::A).client(), &topology.id("Vehicle:linked-local"), 20).await;
    let created = create_entity_map(topology.broker(BrokerRole::A).client(), "type=Vehicle").await;
    assert_status(&created, 201);
    assert!(json_body(&created)["linkedMaps"].is_object(), "the folded map exposes linkedMaps");
    let _forwarded = mock.next_request(FORWARD_TIMEOUT).await;
    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A remote map updates the `entityMap` element with the source id
/// (clause 5.14.4). Confirms REQ-MAP-129.
#[tokio::test]
async fn emq_remote_map_updates_entity_map_element() {
    let topology = topology::single().await.expect("single-broker topology");
    let remote_entity = topology.id("Vehicle:element-remote");
    let mock = query_source(&topology, "registration:element-update", &remote_entity).await;
    seed_vehicle(topology.broker(BrokerRole::A).client(), &topology.id("Vehicle:element-local"), 20).await;
    let created = create_entity_map(topology.broker(BrokerRole::A).client(), "type=Vehicle").await;
    assert_status(&created, 201);
    let entries: Vec<Value> = json_body(&created)["entityMap"]
        .as_object()
        .map(|map| map.values().cloned().collect())
        .unwrap_or_default();
    assert!(entries.iter().all(Value::is_array), "every entityMap element maps an id to a holder list");
    let _forwarded = mock.next_request(FORWARD_TIMEOUT).await;
    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The local map is stored and retrievable (clause 5.14.4). Confirms REQ-MAP-130.
#[tokio::test]
async fn emq_local_map_stored_and_retrievable() {
    let topology = topology::single().await.expect("single-broker topology");
    seed_vehicle(topology.broker(BrokerRole::A).client(), &topology.id("Vehicle:stored"), 20).await;
    let created = create_entity_map(topology.broker(BrokerRole::A).client(), "type=Vehicle").await;
    assert_status(&created, 201);
    let map_id = json_body(&created)["id"]
        .as_str()
        .unwrap_or_else(|| unreachable!("created map has an id"))
        .to_owned();
    let retrieved = topology
        .broker(BrokerRole::A)
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("retrieve entity map");
    assert_status(&retrieved, 200);
    topology.finish().await.expect("topology cleanup");
}

/// Success returns `201` with the `NGSILD-EntityMap` header and an `EntityMap`
/// body (clause 5.14.4). Confirms REQ-MAP-131.
#[tokio::test]
async fn emq_success_returns_201_location_and_body() {
    let topology = topology::single().await.expect("single-broker topology");
    seed_vehicle(topology.broker(BrokerRole::A).client(), &topology.id("Vehicle:success-201"), 20).await;
    let created = create_entity_map(topology.broker(BrokerRole::A).client(), "type=Vehicle").await;
    assert_status(&created, 201);
    assert!(entity_map_header(&created).is_some(), "the response carries the NGSILD-EntityMap header");
    assert_eq!(json_body(&created)["type"].as_str(), Some("EntityMap"), "the response body is an EntityMap");
    topology.finish().await.expect("topology cleanup");
}
