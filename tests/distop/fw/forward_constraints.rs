//! Broker-side enforcement of registration constraints on forwarded requests
//! and the mode/distribution basis (ETSI GS CIM 009 v1.9.1, clauses 4.3.6.3,
//! 4.5.4-4.5.5, 5.7). All `T-MOCK`/`T-MOCK-N`: the mock captures the exact
//! forwarded request so the broker's narrowing is asserted byte-for-byte.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property_with_dataset, vehicle},
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::time::Duration;

/// Budget for a forwarded request to reach the mock context source.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// The path a broker appends to a registration `endpoint` when forwarding an
/// entity query or create (clause 4.3.6.3).
const FORWARDED_ENTITIES: &str = "/ngsi-ld/v1/entities";

/// Locate the entity with the given id inside an entity-array response body.
fn find_entity<'a>(body: &'a Value, id: &str) -> Option<&'a Value> {
    body.as_array()?.iter().find(|entity| entity["id"].as_str() == Some(id))
}

/// A registered `type` constraint narrows a multi-type query to the covered type
/// only: a `Device` the registration does not cover is stripped before the
/// request is forwarded (clause 4.3.6.3). Confirms REQ-FW-1, REQ-FW-6.
#[tokio::test]
async fn forward_scopes_request_to_registered_type_only() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:scope-type"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
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

    let query = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities("type=Vehicle,Device", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&query, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(
        forwarded.uri.path(),
        FORWARDED_ENTITIES,
        "the forwarded request targets the entities collection"
    );
    let forwarded_query = forwarded.uri.query().unwrap_or_default();
    assert!(forwarded_query.contains("Vehicle"), "the covered type survives narrowing: {forwarded_query}");
    assert!(
        !forwarded_query.contains("Device"),
        "the uncovered type is stripped before forwarding: {forwarded_query}"
    );
    assert_eq!(mock.get_count(Method::GET, FORWARDED_ENTITIES), 1, "exactly one forwarded query is issued");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A registration constrained by id and attribute narrows the forwarded request
/// to its covered id and attribute: a second id and an uncovered attribute in
/// the client query never reach the source (clause 4.3.6.3). Confirms REQ-FW-2.
#[tokio::test]
async fn forward_narrows_to_registered_ids_attrs_geofence() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let covered = topology.id("Vehicle:narrow-A1");
    let other = topology.id("Vehicle:narrow-A2");
    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:narrow"), &endpoint)
        .mode("inclusive")
        .entity_id(&covered)
        .property_names(&["speed"])
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

    let query = format!("type=Vehicle&id={covered},{other}&attrs=speed,fuel");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&query, "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let forwarded_query = forwarded.uri.query().unwrap_or_default();
    assert!(forwarded_query.contains("narrow-A1"), "the covered id is forwarded: {forwarded_query}");
    assert!(!forwarded_query.contains("narrow-A2"), "the uncovered id is dropped: {forwarded_query}");
    assert!(forwarded_query.contains("speed"), "the covered attribute is forwarded: {forwarded_query}");
    assert!(!forwarded_query.contains("fuel"), "the uncovered attribute is dropped: {forwarded_query}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A registration constrained by a relationship name narrows the forwarded
/// request to that relationship: a client `attrs` naming the covered
/// relationship and an uncovered property forwards only the covered one, the
/// `relationshipNames` mirror of `forward_narrows_to_registered_ids_attrs_geofence`
/// (clause 4.3.6.3). `isParked` is a Relationship term of the compound test
/// context, so its narrowed name round-trips through compaction.
#[tokio::test]
async fn forward_narrows_to_registered_relationship_names() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let covered = topology.id("Vehicle:narrow-rel");
    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:narrow-rel"), &endpoint)
        .mode("inclusive")
        .entity_id(&covered)
        .relationship_names(&["isParked"])
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

    let query = format!("type=Vehicle&id={covered}&attrs=isParked,speed");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities(&query, "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let forwarded_query = forwarded.uri.query().unwrap_or_default();
    assert!(forwarded_query.contains("isParked"), "the covered relationship is forwarded: {forwarded_query}");
    assert!(
        !forwarded_query.contains("speed"),
        "the uncovered property is dropped before forwarding: {forwarded_query}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Clause 4.3.6.3: an exclusive registration names one entity and its
/// attributes, so a client query naming only the type still has the block's id
/// and attribute added to the forwarded request -- the source is asked for
/// exactly what the registration covers, never the broad `type=Vehicle` alone.
/// Confirms REQ-FW-2.
#[tokio::test]
async fn forward_exclusive_block_names_its_id_and_attribute_for_a_type_only_query() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let covered = topology.id("Vehicle:exclusive-block");
    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:exclusive-block"), &endpoint)
        .mode("exclusive")
        .entity_id(&covered)
        .property_names(&["speed"])
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

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities("type=Vehicle", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let forwarded_query = forwarded.uri.query().unwrap_or_default();
    assert!(
        forwarded_query.contains("exclusive-block"),
        "the exclusive block's id is named in the forwarded request: {forwarded_query}"
    );
    assert!(
        forwarded_query.contains("speed"),
        "the exclusive block's attribute is named in the forwarded request: {forwarded_query}"
    );
    assert!(
        forwarded_query.contains("Vehicle"),
        "the covered type is named in the forwarded request: {forwarded_query}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Constraint enforcement happens at the broker, not the source: the request the
/// source receives through the broker is already narrowed to the covered type,
/// so the source is never asked for data outside the registration (clause
/// 4.3.6.3). Confirms REQ-FW-3.
#[tokio::test]
async fn forward_constraint_enforced_at_broker_not_source() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:broker-enforced"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
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

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities("type=Vehicle,Device", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let forwarded_query = forwarded.uri.query().unwrap_or_default();
    assert!(
        !forwarded_query.contains("Device"),
        "the broker narrows the request so the source never sees the uncovered type: {forwarded_query}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The registration mode governs distribution: an inclusive write is forwarded
/// and held locally, while an auxiliary write is held locally and never
/// forwarded (clause 4.3.6.3, 5.7). Confirms REQ-FW-4.
#[tokio::test]
async fn forward_mode_governs_distribution_and_local_hold() {
    let topology = topology::single().await.expect("single-broker topology");
    let inclusive_mock = ContextSourceAdapter::start().await;
    let auxiliary_mock = ContextSourceAdapter::start().await;
    inclusive_mock.set_stub_with_body(Method::POST, FORWARDED_ENTITIES, 204, &json!({})).await;

    let inclusive_id = topology.id("Vehicle:mode-inclusive");
    let auxiliary_id = topology.id("Vehicle:mode-auxiliary");
    let inclusive_endpoint = inclusive_mock.url("");
    let auxiliary_endpoint = auxiliary_mock.url("");
    let inclusive_registration = CsrBuilder::new(&topology.id("registration:mode-inclusive"), &inclusive_endpoint)
        .mode("inclusive")
        .entity_id(&inclusive_id)
        .property_names(&["speed"])
        .operations(&["createEntity", "updateEntity"])
        .build();
    let auxiliary_registration = CsrBuilder::new(&topology.id("registration:mode-auxiliary"), &auxiliary_endpoint)
        .mode("auxiliary")
        .entity_id(&auxiliary_id)
        .property_names(&["speed"])
        .operations(&["retrieveEntity", "queryEntity"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&inclusive_registration)
            .await
            .expect("inclusive registration"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&auxiliary_registration)
            .await
            .expect("auxiliary registration"),
        201,
    );

    let inclusive_created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .post_entity(&vehicle(&inclusive_id, 40))
        .with_ld_json()
        .send()
        .await
        .expect("inclusive create");
    assert!(inclusive_created.status.is_success(), "inclusive create succeeds: {}", inclusive_created.status);
    let auxiliary_created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .post_entity(&vehicle(&auxiliary_id, 41))
        .with_ld_json()
        .send()
        .await
        .expect("auxiliary create");
    assert!(auxiliary_created.status.is_success(), "auxiliary create succeeds: {}", auxiliary_created.status);

    assert_eq!(inclusive_mock.get_total_count(), 1, "an inclusive write is forwarded to the source");
    assert_eq!(auxiliary_mock.get_total_count(), 0, "an auxiliary write is never forwarded");

    let inclusive_local = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity(&inclusive_id, "local=true", "application/ld+json", None)
        .await
        .expect("inclusive local");
    assert_status(&inclusive_local, 200);
    assert_eq!(
        json_body(&inclusive_local)["speed"]["value"].as_i64(),
        Some(40),
        "the inclusive write is also held locally"
    );
    let auxiliary_local = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity(&auxiliary_id, "local=true", "application/ld+json", None)
        .await
        .expect("auxiliary local");
    assert_status(&auxiliary_local, 200);
    assert_eq!(
        json_body(&auxiliary_local)["speed"]["value"].as_i64(),
        Some(41),
        "the auxiliary write is held locally"
    );

    inclusive_mock.shutdown().await;
    auxiliary_mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The same attribute served by two sources with distinct `datasetId` values is
/// merged into two instances, neither overwriting the other (clause 4.5.5).
/// Confirms REQ-FW-5.
#[tokio::test]
async fn forward_same_attribute_two_sources_distinguished_by_dataset_id() {
    let topology = topology::single().await.expect("single-broker topology");
    let first_mock = ContextSourceAdapter::start().await;
    let second_mock = ContextSourceAdapter::start().await;

    let entity_id = topology.id("Vehicle:dataset-A1");
    let first_body = json!([entity(
        &entity_id,
        "Vehicle",
        &[("speed", property_with_dataset(json!(50), "urn:ngsi-ld:dataset:first"))]
    )]);
    let second_body = json!([entity(
        &entity_id,
        "Vehicle",
        &[("speed", property_with_dataset(json!(70), "urn:ngsi-ld:dataset:second"))]
    )]);
    first_mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &first_body).await;
    second_mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &second_body).await;

    let first_endpoint = first_mock.url("");
    let second_endpoint = second_mock.url("");
    for (suffix, endpoint) in [("first", &first_endpoint), ("second", &second_endpoint)] {
        let registration = CsrBuilder::new(&topology.id(&format!("registration:dataset-{suffix}")), endpoint)
            .mode("inclusive")
            .entity_id(&entity_id)
            .property_names(&["speed"])
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
    let merged = find_entity(&body, &entity_id).expect("the merged entity is present");
    let instances = merged["speed"].as_array().expect("the two-source attribute merges into an array");
    assert_eq!(instances.len(), 2, "each datasetId contributes one instance: {}", merged["speed"]);

    first_mock.shutdown().await;
    second_mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A query is not forwarded to a source whose registration does not advertise
/// the `queryEntity` operation (clause 4.3.6.3). Confirms REQ-FW-7.
#[tokio::test]
async fn forward_omits_operation_not_supported_by_source() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:retrieve-only"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
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

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities("type=Vehicle", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    assert_eq!(
        mock.get_count(Method::GET, FORWARDED_ENTITIES),
        0,
        "a query is not forwarded to a retrieve-only source"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// With no representation option in the client request, the forwarded request
/// carries no `keyValues`/`concise`/`simplified` option: the default normalized
/// form is requested from the source (clause 4.3.6.3). Confirms REQ-FW-8.
#[tokio::test]
async fn forward_default_representation_is_normalized() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:default-format"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
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

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities("type=Vehicle", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let forwarded_query = forwarded.uri.query().unwrap_or_default();
    assert!(
        !forwarded_query.contains("keyValues"),
        "no keyValues option is forwarded by default: {forwarded_query}"
    );
    assert!(
        !forwarded_query.contains("concise"),
        "no concise option is forwarded by default: {forwarded_query}"
    );
    assert!(
        !forwarded_query.contains("simplified"),
        "no simplified option is forwarded by default: {forwarded_query}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A non-normalized representation is optional at the source: a source declining
/// `keyValues` with 501 does not fail the client request with a 5xx (clause
/// 4.3.6.3). Confirms REQ-FW-9.
#[tokio::test]
async fn forward_non_normalized_format_optional_no_failure() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub(Method::GET, FORWARDED_ENTITIES, 501).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:keyvalues-optional"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
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

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities("type=Vehicle&options=keyValues", "application/ld+json", None)
        .await
        .expect("query");
    assert!(
        response.status.as_u16() < 500,
        "a source declining keyValues does not fail the client with a 5xx: {}",
        response.status
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A source that returns entities without `createdAt`/`modifiedAt` under
/// `sysAttrs` does not cause an error: the system attributes are simply absent
/// (clause 4.3.6.3). Confirms REQ-FW-10.
#[tokio::test]
async fn forward_system_attrs_absent_without_error() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:sysattrs");
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([vehicle(&entity_id, 55)]))
        .await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:sysattrs"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
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

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities("type=Vehicle&options=sysAttrs", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let body = json_body(&response);
    let entity = find_entity(&body, &entity_id).expect("the source entity is present");
    assert!(entity.get("createdAt").is_none(), "an absent createdAt is tolerated: {entity}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
