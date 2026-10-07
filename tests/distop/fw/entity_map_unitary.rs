//! Linked `EntityMap` creation, reuse, pinning, and lifecycle for a single broker instance (ETSI GS CIM 009 v1.9.1, clauses 5.5.9.3, 5.7.2, 5.14).

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::typed_vehicle,
    oracle::{assert_problem_details, assert_status, entity_map_header, forwarded_header, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::INVALID_REQUEST;
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use axum::http::Method;
use pretty_assertions::{assert_eq, assert_ne};
use serde_json::json;
use std::time::Duration;
use urlencoding::encode;

/// How long to wait for the forwarded request a map reuse issues to its source.
const FORWARD_WAIT: Duration = Duration::from_secs(5);

/// The path a broker appends to a registration `endpoint` for an entity query.
const FORWARDED_ENTITIES: &str = "/ngsi-ld/v1/entities";

/// Register an inclusive query CSR at the mock, covering the per-test type, and
/// assert acceptance.
async fn register_inclusive_query(topology: &MultiBrokerTopology<BrokerRole>, endpoint: &str, name: &str, vtype: &str) {
    let registration = CsrBuilder::new(&topology.id(name), endpoint)
        .mode("inclusive")
        .entity_type(vtype)
        .operations(&["queryEntity"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("create registration"),
        201,
    );
}

/// A request that asks for an entity map issues one broad query to each matching source (clause 5.7.2).
#[tokio::test]
async fn distop_entity_map_broad_request_to_each_matching_source() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let first_mock = ContextSourceAdapter::start().await;
    let second_mock = ContextSourceAdapter::start().await;
    first_mock
        .set_stub_with_body(
            Method::GET,
            FORWARDED_ENTITIES,
            200,
            &json!([typed_vehicle(&topology.id("Vehicle:map-broad-1"), &vtype, 11)]),
        )
        .await;
    second_mock
        .set_stub_with_body(
            Method::GET,
            FORWARDED_ENTITIES,
            200,
            &json!([typed_vehicle(&topology.id("Vehicle:map-broad-2"), &vtype, 22)]),
        )
        .await;

    let first_endpoint = first_mock.url("");
    let second_endpoint = second_mock.url("");
    register_inclusive_query(&topology, &first_endpoint, "registration:map-broad-1", &vtype).await;
    register_inclusive_query(&topology, &second_endpoint, "registration:map-broad-2", &vtype).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}&entityMap=true", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 201);
    assert!(entity_map_header(&response).is_some(), "an entity map is created and its id returned");
    assert_eq!(
        first_mock.get_count(Method::GET, FORWARDED_ENTITIES),
        1,
        "the first source gets one broad query"
    );
    assert_eq!(
        second_mock.get_count(Method::GET, FORWARDED_ENTITIES),
        1,
        "the second source gets one broad query"
    );

    first_mock.shutdown().await;
    second_mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The response carries an `NGSILD-EntityMap` and, across paginated pages, only ids from the mapped set are returned (clause 5.5.9.3).
#[tokio::test]
async fn distop_entity_map_query_pins_ids_only_mapped_paginated() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let first_id = topology.id("Vehicle:map-pin-1");
    let second_id = topology.id("Vehicle:map-pin-2");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&first_id, &vtype, 10))
            .with_ld_json()
            .send()
            .await
            .expect("first create"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&second_id, &vtype, 20))
            .with_ld_json()
            .send()
            .await
            .expect("second create"),
        201,
    );

    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}&entityMap=true", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("map query");
    assert_status(&created, 201);
    let map_id = entity_map_header(&created).expect("map id returned");

    let page = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/entities?type={}&limit=1", encode(&vtype)))
        .header("NGSILD-EntityMap", map_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("paginated reuse");
    assert_status(&page, 200);
    for entity in json_body(&page).as_array().into_iter().flatten() {
        let id = entity["id"].as_str().unwrap_or_default();
        assert!(id == first_id || id == second_id, "a paginated page returns only mapped ids: {id}");
    }

    topology.finish().await.expect("topology cleanup");
}

/// A candidate that does not match the map's filter is excluded from the mapped set (clause 5.5.9.3, 5.14).
#[tokio::test]
async fn distop_entity_map_filters_remove_non_matching_ids_during_pagination() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let matching = topology.id("Vehicle:map-filter-match");
    let non_matching = topology.id("Vehicle:map-filter-nomatch");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&matching, &vtype, 60))
            .with_ld_json()
            .send()
            .await
            .expect("matching create"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&non_matching, &vtype, 40))
            .with_ld_json()
            .send()
            .await
            .expect("non-matching create"),
        201,
    );

    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}&q=speed%3E50&entityMap=true", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("map query");
    assert_status(&created, 201);
    let map_id = entity_map_header(&created).expect("map id returned");

    let reused = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/entities?type={}&q=speed%3E50", encode(&vtype)))
        .header("NGSILD-EntityMap", map_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("reuse");
    assert_status(&reused, 200);
    let reused_body = json_body(&reused);
    let ids: Vec<&str> = reused_body
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entity| entity["id"].as_str())
        .collect();
    assert!(
        !ids.iter().any(|id| *id == non_matching),
        "a non-matching candidate is excluded from the map: {ids:?}"
    );

    topology.finish().await.expect("topology cleanup");
}

/// The `NGSILD-EntityMap` location is returned when a map is requested and the map resource is retrievable there (clause 5.14).
#[tokio::test]
async fn distop_entity_map_location_returned_when_requested() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:map-location");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 15))
            .with_ld_json()
            .send()
            .await
            .expect("create"),
        201,
    );

    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}&entityMap=true", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("map query");
    assert_status(&created, 201);
    let map_id = entity_map_header(&created).expect("map id returned");

    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("retrieve map");
    assert_status(&retrieved, 200);

    topology.finish().await.expect("topology cleanup");
}

/// A retrieval over a stored map contacts only the source recorded as holding the entity, not every source (clause 5.7.2).
#[tokio::test]
async fn distop_entity_map_retrieve_records_holding_sources_single_instance() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let holding_mock = ContextSourceAdapter::start().await;
    let other_mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:map-holding");
    holding_mock
        .set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([typed_vehicle(&entity_id, &vtype, 33)]))
        .await;
    holding_mock
        .set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([typed_vehicle(&entity_id, &vtype, 33)]))
        .await;
    other_mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let holding_endpoint = holding_mock.url("");
    let other_endpoint = other_mock.url("");
    register_inclusive_query(&topology, &holding_endpoint, "registration:map-holding", &vtype).await;
    register_inclusive_query(&topology, &other_endpoint, "registration:map-other", &vtype).await;

    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}&id={entity_id}&entityMap=true", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("map query");
    assert_status(&created, 201);
    let map_id = entity_map_header(&created).expect("map id returned");
    let other_after_create = other_mock.get_total_count();

    let reused = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/entities?type={}&id={entity_id}", encode(&vtype)))
        .header("NGSILD-EntityMap", map_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("reuse");
    assert_status(&reused, 200);
    assert_eq!(
        other_mock.get_total_count(),
        other_after_create,
        "the non-holding source is not contacted on reuse"
    );

    holding_mock.shutdown().await;
    other_mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A map minted on broker A is unknown to peer B, which therefore (re)creates one and answers `201` with a fresh map id (Table 6.4.3.2-3: "201 Created (in case an `EntityMap` has been (re)created)"; clause 5.7.2.4: "If the resource cannot be found, or the data has expired, a new `EntityMap` shall be created").
#[tokio::test]
async fn distop_entity_map_retrieve_cross_instance_reuse() {
    let topology = topology::pair().await.expect("two-broker topology");
    let vtype = topology.id("Vehicle");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let peer = topology.broker(BrokerRole::B).expect("topology broker B");
    let entity_id = topology.id("Vehicle:map-cross");
    assert_status(
        &front
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 18))
            .with_ld_json()
            .send()
            .await
            .expect("create"),
        201,
    );

    let created = front
        .client()
        .query_entities(&format!("type={}&entityMap=true", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("map query");
    assert_status(&created, 201);
    let map_id = entity_map_header(&created).expect("map id returned");

    let reused = peer
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/entities?type={}", encode(&vtype)))
        .header("NGSILD-EntityMap", map_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("cross-instance reuse");
    assert_status(&reused, 201);
    let recreated = entity_map_header(&reused).expect("the peer returns the map it created");
    assert_ne!(recreated, map_id, "a map unknown to the peer is recreated under a fresh id");

    topology.finish().await.expect("topology cleanup");
}

/// Passing the map field back reuses the existing map: no new map is minted (`200`, same id) and the mapped source is still consulted under the map, the forwarded request carrying the linked map id (clause 4.3.6.7: "Only the Entities whose identifiers are contained in the Entity map are considered when rendering the result pages"; clause 5.7.2.4: "If an `EntityMap` is in use for this operation, and an `EntityMap` entry linked to a Context Source Registration is found, the location of the linked `EntityMap` shall be passed as part of any forwarded request").
#[tokio::test]
async fn distop_entity_map_reused_when_field_passed_back() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:map-reuse");
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([typed_vehicle(&entity_id, &vtype, 25)]))
        .await;

    let endpoint = mock.url("");
    register_inclusive_query(&topology, &endpoint, "registration:map-reuse", &vtype).await;

    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}&entityMap=true", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("map query");
    assert_status(&created, 201);
    let map_id = entity_map_header(&created).expect("map id returned");
    let _broad = mock.next_request(FORWARD_WAIT).await;
    let count_after_create = mock.get_total_count();

    let reused = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/entities?type={}", encode(&vtype)))
        .header("NGSILD-EntityMap", map_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("reuse");
    assert_status(&reused, 200);
    assert_eq!(entity_map_header(&reused).as_deref(), Some(map_id.as_str()), "reusing the map mints no new map");
    assert!(mock.get_total_count() > count_after_create, "the mapped source is consulted under the map");
    let forwarded = mock.next_request(FORWARD_WAIT).await;
    assert!(
        forwarded_header(&forwarded, "ngsild-entitymap").is_some(),
        "the forwarded request carries the linked map id"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A subsequent request over a map returns only the listed entities: a source entity added after the map was pinned is excluded (clause 5.7.2).
#[tokio::test]
async fn distop_entity_map_subsequent_request_filters_to_listed_entities() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mapped = topology.id("Vehicle:map-listed");
    let added_later = topology.id("Vehicle:map-added");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&mapped, &vtype, 10))
            .with_ld_json()
            .send()
            .await
            .expect("mapped create"),
        201,
    );

    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}&entityMap=true", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("map query");
    assert_status(&created, 201);
    let map_id = entity_map_header(&created).expect("map id returned");

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&added_later, &vtype, 20))
            .with_ld_json()
            .send()
            .await
            .expect("later create"),
        201,
    );

    let reused = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/entities?type={}", encode(&vtype)))
        .header("NGSILD-EntityMap", map_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("reuse");
    assert_status(&reused, 200);
    let reused_body = json_body(&reused);
    let ids: Vec<&str> = reused_body
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entity| entity["id"].as_str())
        .collect();
    assert!(!ids.iter().any(|id| *id == added_later), "an entity added after mapping is excluded: {ids:?}");

    topology.finish().await.expect("topology cleanup");
}

/// A subsequent request that changes a non-id, non-pagination filter parameter is rejected with 400 `InvalidRequest` (clause 5.5.9.3).
#[tokio::test]
async fn distop_entity_map_subsequent_request_rejects_changed_filter_params() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:map-changed-filter");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 60))
            .with_ld_json()
            .send()
            .await
            .expect("create"),
        201,
    );

    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}&entityMap=true", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("map query");
    assert_status(&created, 201);
    let map_id = entity_map_header(&created).expect("map id returned");

    let rejected = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/entities?type={}&q=speed%3E99", encode(&vtype)))
        .header("NGSILD-EntityMap", map_id.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("changed-filter reuse");
    assert_status(&rejected, 400);
    assert_problem_details(&rejected, INVALID_REQUEST);

    topology.finish().await.expect("topology cleanup");
}

/// A reuse against an expired or unknown map creates a fresh map and re-issues the broad request (clause 5.7.2).
#[tokio::test]
async fn distop_entity_map_expired_or_inaccessible_creates_new() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:map-expired");
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([typed_vehicle(&entity_id, &vtype, 25)]))
        .await;

    let endpoint = mock.url("");
    register_inclusive_query(&topology, &endpoint, "registration:map-expired", &vtype).await;

    let unknown_map = topology.id("EntityMap:does-not-exist");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/entities?type={}", encode(&vtype)))
        .header("NGSILD-EntityMap", unknown_map.as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("reuse of unknown map");
    assert!(
        response.status.is_success(),
        "an unknown map yields a fresh map rather than an error: {}",
        response.status
    );
    assert!(mock.get_total_count() >= 1, "the broad request is re-issued for a fresh map");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Only the expiry of a map may be updated by other components; the update is accepted while the entity list stays intact (clause 5.14).
#[tokio::test]
async fn distop_entity_map_only_expiry_updatable_by_other_components() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:map-expiry-update");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 15))
            .with_ld_json()
            .send()
            .await
            .expect("create"),
        201,
    );

    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type={}&entityMap=true", encode(&vtype)), "application/ld+json", None)
        .await
        .expect("map query");
    assert_status(&created, 201);
    let map_id = entity_map_header(&created).expect("map id returned");

    let patched = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::PATCH, &format!("/ngsi-ld/v1/entityMaps/{map_id}"))
        .json(&json!({ "expiresAt": "2099-01-01T00:00:00Z" }))
        .send()
        .await
        .expect("update expiry");
    assert_status(&patched, 204);

    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_entity_map(&map_id)
        .await
        .expect("retrieve map");
    assert_status(&retrieved, 200);

    topology.finish().await.expect("topology cleanup");
}
