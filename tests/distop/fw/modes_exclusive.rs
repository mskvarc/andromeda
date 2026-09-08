//! Exclusive mode: registered attributes are served only from the source, never
//! held locally; an exclusive footprint must be an exact id with attributes and
//! may not overlap another exclusive or redirect registration (ETSI GS CIM 009
//! v1.9.1, clauses 4.3.6.3, 5.2.9, 5.7). Rejection rows are `T-LOCAL`;
//! forwarding rows are `T-MOCK`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property, vehicle},
    oracle::{assert_problem_details, assert_status, entity_map_header, json_body},
    reg::PEER_ENDPOINT,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::problem_detail::{BAD_REQUEST_DATA, CONFLICT};
use axum::http::Method;
use serde_json::{Value, json};
use std::time::Duration;

/// Budget for a forwarded request to reach the mock context source.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// The path a broker appends to a registration `endpoint` for an entity query.
const FORWARDED_ENTITIES: &str = "/ngsi-ld/v1/entities";

/// Locate the entity with the given id inside an entity-array response body.
fn find_entity<'a>(body: &'a Value, id: &str) -> Option<&'a Value> {
    body.as_array()?.iter().find(|entity| entity["id"].as_str() == Some(id))
}

/// An exclusive registration serves its registered attribute only from the
/// source: the provision is forwarded, the attribute is absent from the local
/// store, and the retrieve returns it from the source (clause 4.3.6.3).
/// Confirms REQ-FW-19, REQ-FW-21.
#[tokio::test]
async fn distop_entity_exclusive_registered_attrs_served_only_from_source_none_local() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:exclusive-source");
    mock.set_stub_with_body(Method::POST, FORWARDED_ENTITIES, 201, &json!({})).await;
    mock.set_stub_with_body(Method::GET, &format!("{FORWARDED_ENTITIES}/{entity_id}"), 200, &vehicle(&entity_id, 88))
        .await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:exclusive-source"), &endpoint)
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["createEntity", "retrieveEntity"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&registration)
            .await
            .expect("create registration"),
        201,
    );

    let created = topology
        .broker(BrokerRole::A)
        .client()
        .post_entity(&vehicle(&entity_id, 88))
        .with_ld_json()
        .send()
        .await
        .expect("provision");
    assert!(created.status.is_success(), "the exclusive provision succeeds: {}", created.status);
    assert!(mock.get_total_count().await >= 1, "the provision is forwarded to the source");

    let retrieved = topology
        .broker(BrokerRole::A)
        .client()
        .get_entity(&entity_id, "", "application/ld+json", None)
        .await
        .expect("retrieve");
    assert_status(&retrieved, 200);
    assert_eq!(
        json_body(&retrieved)["speed"]["value"].as_i64(),
        Some(88),
        "the registered attribute is served from the source"
    );

    let local = topology
        .broker(BrokerRole::A)
        .client()
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local retrieve");
    assert!(
        local.status.as_u16() == 404 || json_body(&local)["speed"].is_null(),
        "the registered attribute is not held locally: {}",
        String::from_utf8_lossy(&local.body)
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Attributes outside the exclusive footprint may be held locally: the
/// registered attribute is forwarded and the unregistered attribute stays local
/// (clause 4.3.6.3). Confirms REQ-FW-20.
#[tokio::test]
async fn distop_entity_exclusive_unregistered_attrs_may_be_held_locally() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:exclusive-mixed");
    mock.set_stub_with_body(Method::POST, FORWARDED_ENTITIES, 201, &json!({})).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:exclusive-mixed"), &endpoint)
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["createEntity"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&registration)
            .await
            .expect("create registration"),
        201,
    );

    let body = entity(&entity_id, "Vehicle", &[("speed", property(json!(40))), ("fuel", property(json!(50)))]);
    let created = topology
        .broker(BrokerRole::A)
        .client()
        .post_entity(&body)
        .with_ld_json()
        .send()
        .await
        .expect("provision");
    assert!(created.status.is_success(), "the mixed provision succeeds: {}", created.status);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let forwarded_body: Value = forwarded.json().expect("forwarded create body is JSON");
    assert!(forwarded_body.get("speed").is_some(), "the registered attribute is forwarded: {forwarded_body}");
    assert!(
        forwarded_body.get("fuel").is_none(),
        "the unregistered attribute is not forwarded: {forwarded_body}"
    );

    let local = topology
        .broker(BrokerRole::A)
        .client()
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local retrieve");
    assert_status(&local, 200);
    let local_body = json_body(&local);
    assert_eq!(local_body["fuel"]["value"].as_i64(), Some(50), "the unregistered attribute is held locally");
    assert!(local_body["speed"].is_null(), "the registered attribute is not held locally: {local_body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A second exclusive registration overlapping an existing one on the same id
/// and attribute is rejected with 409 Conflict (clause 5.2.9). Confirms
/// REQ-FW-22, REQ-FW-25.
#[tokio::test]
async fn distop_entity_exclusive_second_overlapping_exclusive_rejected_conflict() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:exclusive-conflict");

    let first = CsrBuilder::new(&topology.id("registration:exclusive-first"), PEER_ENDPOINT)
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&first)
            .await
            .expect("first registration"),
        201,
    );

    let second = CsrBuilder::new(&topology.id("registration:exclusive-second"), PEER_ENDPOINT)
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .build();
    let conflict = topology
        .broker(BrokerRole::A)
        .client()
        .create_registration(&second)
        .await
        .expect("second registration");
    assert_status(&conflict, 409);
    assert_problem_details(&conflict, CONFLICT);

    topology.finish().await.expect("topology cleanup");
}

/// A redirect registration overlapping an existing exclusive one on the same id
/// and attribute is rejected with 409 Conflict (clause 5.2.9). Confirms
/// REQ-FW-25.
#[tokio::test]
async fn distop_entity_exclusive_then_redirect_same_id_attrs_rejected() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:exclusive-redirect-conflict");

    let exclusive = CsrBuilder::new(&topology.id("registration:exclusive-base"), PEER_ENDPOINT)
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&exclusive)
            .await
            .expect("exclusive registration"),
        201,
    );

    let redirect = CsrBuilder::new(&topology.id("registration:redirect-overlap"), PEER_ENDPOINT)
        .mode("redirect")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .build();
    let conflict = topology
        .broker(BrokerRole::A)
        .client()
        .create_registration(&redirect)
        .await
        .expect("redirect registration");
    assert_status(&conflict, 409);
    assert_problem_details(&conflict, CONFLICT);

    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration expressed by `idPattern` rather than an exact id is
/// rejected with 400 `BadRequestData` (clause 5.2.9). Confirms REQ-FW-23.
#[tokio::test]
async fn distop_entity_exclusive_registration_with_id_pattern_rejected() {
    let topology = topology::single().await.expect("single-broker topology");
    let registration = CsrBuilder::new(&topology.id("registration:exclusive-idpattern"), PEER_ENDPOINT)
        .mode("exclusive")
        .id_pattern("urn:ngsi-ld:Vehicle:.*")
        .property_names(&["speed"])
        .build();
    let rejected = topology
        .broker(BrokerRole::A)
        .client()
        .create_registration(&registration)
        .await
        .expect("create registration");
    assert_status(&rejected, 400);
    assert_problem_details(&rejected, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration by type only, without an exact id, is rejected with
/// 400 `BadRequestData` (clause 5.2.9). Confirms REQ-FW-23.
#[tokio::test]
async fn distop_entity_exclusive_registration_type_only_rejected() {
    let topology = topology::single().await.expect("single-broker topology");
    let registration = CsrBuilder::new(&topology.id("registration:exclusive-typeonly"), PEER_ENDPOINT)
        .mode("exclusive")
        .entity_type("Vehicle")
        .property_names(&["speed"])
        .build();
    let rejected = topology
        .broker(BrokerRole::A)
        .client()
        .create_registration(&registration)
        .await
        .expect("create registration");
    assert_status(&rejected, 400);
    assert_problem_details(&rejected, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// An exclusive registration with an exact id but no property or relationship
/// names is rejected with 400 `BadRequestData` (clause 5.2.9). Confirms REQ-FW-24.
#[tokio::test]
async fn distop_entity_exclusive_registration_without_attributes_rejected() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:exclusive-noattrs");
    let registration = CsrBuilder::new(&topology.id("registration:exclusive-noattrs"), PEER_ENDPOINT)
        .mode("exclusive")
        .entity_id(&entity_id)
        .build();
    let rejected = topology
        .broker(BrokerRole::A)
        .client()
        .create_registration(&registration)
        .await
        .expect("create registration");
    assert_status(&rejected, 400);
    assert_problem_details(&rejected, BAD_REQUEST_DATA);
    topology.finish().await.expect("topology cleanup");
}

/// A query matching a single exclusive registration reaches the source by
/// inferred relevance, with no entity map requested and none returned (clause
/// 4.3.6.3, 6.3.15). Confirms REQ-FW-52.
#[tokio::test]
async fn distop_entity_exclusive_relevance_inferred_without_entity_map() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:exclusive-relevance");
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([vehicle(&entity_id, 33)]))
        .await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:exclusive-relevance"), &endpoint)
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["queryEntity"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&registration)
            .await
            .expect("create registration"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .query_entities(&format!("type=Vehicle&id={entity_id}"), "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    assert!(entity_map_header(&response).is_none(), "no entity map is returned when none was requested");
    assert!(
        mock.get_count(Method::GET, FORWARDED_ENTITIES).await >= 1,
        "the source is contacted by inferred relevance"
    );
    let body = json_body(&response);
    assert!(find_entity(&body, &entity_id).is_some(), "the source result is returned: {body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
