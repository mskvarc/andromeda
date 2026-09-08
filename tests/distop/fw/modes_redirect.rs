//! Redirect mode: registered data is obtained externally and never held
//! locally; a redirect footprint may be by id, idPattern, type, or attributes
//! only, and multiple overlapping redirects fan out to every source (ETSI GS
//! CIM 009 v1.9.1, clauses 4.3.6.3, 5.2.9, 5.7). Registration rows are
//! `T-LOCAL`; forwarding rows are `T-MOCK`/`T-MOCK-N`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property, vehicle},
    oracle::{assert_status, json_body},
    reg::PEER_ENDPOINT,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
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

/// Register a redirect CSR and assert it is accepted with 201.
async fn assert_redirect_accepted(topology: &MultiBrokerTopology<BrokerRole>, registration: &Value) {
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(registration)
        .await
        .expect("create registration");
    assert_status(&created, 201);
}

/// A redirect registration obtains its registered attribute from the source with
/// nothing held locally and no local conflict (clause 4.3.6.3). Confirms
/// REQ-FW-26.
#[tokio::test]
async fn distop_entity_redirect_registered_data_obtained_externally() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:redirect-external");
    mock.set_stub_with_body(Method::GET, &format!("{FORWARDED_ENTITIES}/{entity_id}"), 200, &vehicle(&entity_id, 77))
        .await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:redirect-external"), &endpoint)
        .mode("redirect")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity"])
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

    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity(&entity_id, "", "application/ld+json", None)
        .await
        .expect("retrieve");
    assert_status(&retrieved, 200);
    assert_eq!(
        json_body(&retrieved)["speed"]["value"].as_i64(),
        Some(77),
        "the registered attribute is obtained externally"
    );

    let local = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity(&entity_id, "local=true", "application/ld+json", None)
        .await
        .expect("local retrieve");
    assert!(
        local.status.as_u16() == 404 || json_body(&local)["speed"].is_null(),
        "the redirected attribute is not held locally: {}",
        String::from_utf8_lossy(&local.body)
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A redirect registration by exact id is accepted (clause 5.2.9). Confirms
/// REQ-FW-27.
#[tokio::test]
async fn distop_entity_redirect_registration_by_id_accepted() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:redirect-by-id");
    let registration = CsrBuilder::new(&topology.id("registration:redirect-by-id"), PEER_ENDPOINT)
        .mode("redirect")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .build();
    assert_redirect_accepted(&topology, &registration).await;
    topology.finish().await.expect("topology cleanup");
}

/// A redirect registration by idPattern is accepted (clause 5.2.9). Confirms
/// REQ-FW-27.
#[tokio::test]
async fn distop_entity_redirect_registration_by_id_pattern_accepted() {
    let topology = topology::single().await.expect("single-broker topology");
    let registration = CsrBuilder::new(&topology.id("registration:redirect-by-idpattern"), PEER_ENDPOINT)
        .mode("redirect")
        .id_pattern("urn:ngsi-ld:Vehicle:.*")
        .build();
    assert_redirect_accepted(&topology, &registration).await;
    topology.finish().await.expect("topology cleanup");
}

/// A redirect registration by type only is accepted (clause 5.2.9). Confirms
/// REQ-FW-27.
#[tokio::test]
async fn distop_entity_redirect_registration_type_only_accepted() {
    let topology = topology::single().await.expect("single-broker topology");
    let registration = CsrBuilder::new(&topology.id("registration:redirect-type-only"), PEER_ENDPOINT)
        .mode("redirect")
        .entity_type("Vehicle")
        .build();
    assert_redirect_accepted(&topology, &registration).await;
    topology.finish().await.expect("topology cleanup");
}

/// A redirect registration by attributes only, with no entity clause, is
/// accepted (clause 5.2.9). Confirms REQ-FW-27.
#[tokio::test]
async fn distop_entity_redirect_registration_attributes_only_accepted() {
    let topology = topology::single().await.expect("single-broker topology");
    let registration = CsrBuilder::new(&topology.id("registration:redirect-attrs-only"), PEER_ENDPOINT)
        .mode("redirect")
        .without_entity()
        .property_names(&["speed"])
        .build();
    assert_redirect_accepted(&topology, &registration).await;
    topology.finish().await.expect("topology cleanup");
}

/// A redirect registration for a whole entity by id, with no attribute clause,
/// forwards a retrieve carrying no `attrs` and returns every source attribute
/// with nothing local (clause 4.3.6.3). Confirms REQ-FW-28.
#[tokio::test]
async fn distop_entity_redirect_whole_entity_by_id_forwards_all_attributes() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:redirect-whole");
    let source_entity = entity(&entity_id, "Vehicle", &[("speed", property(json!(65))), ("fuel", property(json!(40)))]);
    mock.set_stub_with_body(Method::GET, &format!("{FORWARDED_ENTITIES}/{entity_id}"), 200, &source_entity)
        .await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:redirect-whole"), &endpoint)
        .mode("redirect")
        .entity_id(&entity_id)
        .operations(&["retrieveEntity"])
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

    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity(&entity_id, "", "application/ld+json", None)
        .await
        .expect("retrieve");
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);
    assert_eq!(body["speed"]["value"].as_i64(), Some(65), "the whole-entity redirect returns speed");
    assert_eq!(body["fuel"]["value"].as_i64(), Some(40), "the whole-entity redirect returns fuel");

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let forwarded_query = forwarded.uri.query().unwrap_or_default();
    assert!(
        !forwarded_query.contains("attrs"),
        "a whole-entity redirect forwards no attrs selector: {forwarded_query}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Multiple overlapping redirect registrations fan out to every source, each
/// receiving one request, their results merged with no conflicting local data
/// (clause 4.3.6.3). Confirms REQ-FW-29.
#[tokio::test]
async fn distop_entity_redirect_multiple_overlapping_fan_out_to_all_sources() {
    let topology = topology::single().await.expect("single-broker topology");
    let first_mock = ContextSourceAdapter::start().await;
    let second_mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:redirect-fanout");
    first_mock
        .set_stub_with_body(
            Method::GET,
            FORWARDED_ENTITIES,
            200,
            &json!([entity(&entity_id, "Vehicle", &[("speed", property(json!(50)))])]),
        )
        .await;
    second_mock
        .set_stub_with_body(
            Method::GET,
            FORWARDED_ENTITIES,
            200,
            &json!([entity(&entity_id, "Vehicle", &[("fuel", property(json!(35)))])]),
        )
        .await;

    let first_endpoint = first_mock.url("");
    let second_endpoint = second_mock.url("");
    for (suffix, endpoint, attribute) in [("speed", &first_endpoint, "speed"), ("fuel", &second_endpoint, "fuel")] {
        let registration = CsrBuilder::new(&topology.id(&format!("registration:redirect-fanout-{suffix}")), endpoint)
            .mode("redirect")
            .entity_id(&entity_id)
            .property_names(&[attribute])
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

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&format!("type=Vehicle&id={entity_id}"), "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let body = json_body(&response);
    let merged = find_entity(&body, &entity_id).expect("the entity is present");
    assert_eq!(merged["speed"]["value"].as_i64(), Some(50), "the first source contributes speed");
    assert_eq!(merged["fuel"]["value"].as_i64(), Some(35), "the second source contributes fuel");
    assert_eq!(first_mock.get_count(Method::GET, FORWARDED_ENTITIES), 1, "the first source is queried once");
    assert_eq!(second_mock.get_count(Method::GET, FORWARDED_ENTITIES), 1, "the second source is queried once");

    first_mock.shutdown().await;
    second_mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
