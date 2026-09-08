//! Version negotiation with older context sources: the broker rejects a
//! malformed source payload, requests a target version, records the returned
//! version, and collates payloads that omit newer members (ETSI GS CIM 009
//! v1.9.1, clause 4.3.6.8). All `T-MOCK`/`T-MOCK-N`. The version-negotiation
//! rows are `UNKNOWN` and classify on first run.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    oracle::{assert_status, forwarded_header, has_warning, json_body},
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

/// The `contextSourceInfo` key by which a registration declares the NGSI-LD
/// conformance version of its source.
const CONFORMANCE_KEY: &str = "ngsildConformance";

/// Locate the entity with the given id inside an entity-array response body.
fn find_entity<'a>(body: &'a Value, id: &str) -> Option<&'a Value> {
    body.as_array()?.iter().find(|entity| entity["id"].as_str() == Some(id))
}

/// Register an inclusive query CSR at the mock and assert acceptance.
async fn register_inclusive(topology: &MultiBrokerTopology<BrokerRole>, endpoint: &str, name: &str, conformance: Option<&str>) {
    let mut builder = CsrBuilder::new(&topology.id(name), endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["queryEntity"]);
    if let Some(version) = conformance {
        builder = builder.context_source_info(CONFORMANCE_KEY, version);
    }
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&builder.build())
            .await
            .expect("create registration"),
        201,
    );
}

/// A malformed source payload is excluded from the aggregate rather than failing
/// the request: the response is 200 with a 111 warning (clause 4.3.6.8).
/// Confirms REQ-FW-56.
#[tokio::test]
async fn distop_entity_bc_broker_rejects_malformed_source_payload() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_raw_body(Method::GET, FORWARDED_ENTITIES, 200, "{ this is not valid json", "application/json")
        .await;

    let endpoint = mock.url("");
    register_inclusive(&topology, &endpoint, "registration:bc-malformed", None).await;

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .query_entities("type=Vehicle", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    assert!(has_warning(&response, 111), "a malformed source payload yields a 111 warning");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The broker indicates a target version on the forwarded request so a source
/// can amend its response to that version (clause 4.3.6.8). Confirms REQ-FW-57.
#[tokio::test]
async fn distop_entity_bc_broker_requests_target_version_source_amends() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let endpoint = mock.url("");
    register_inclusive(&topology, &endpoint, "registration:bc-target-version", None).await;

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .query_entities("type=Vehicle", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let prefer = forwarded_header(&forwarded, "prefer").unwrap_or_default();
    assert!(
        prefer.contains("ngsi-ld="),
        "the forwarded request indicates the target NGSI-LD version: {prefer:?}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The requested version value is in major.minor form (clause 4.3.6.8). Confirms
/// REQ-FW-58.
#[tokio::test]
async fn distop_entity_bc_version_values_are_major_minor() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let endpoint = mock.url("");
    register_inclusive(&topology, &endpoint, "registration:bc-version-form", None).await;

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .query_entities("type=Vehicle", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let prefer = forwarded_header(&forwarded, "prefer").unwrap_or_default();
    let version = prefer.split("ngsi-ld=").nth(1).map(|rest| rest.trim()).unwrap_or_default();
    let segments = version.split('.').count();
    assert_eq!(segments, 2, "the requested version is major.minor: {prefer:?}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The broker reads and records the version a source indicates it returned, so a
/// source-provided entity is still integrated (clause 4.3.6.8). Confirms
/// REQ-FW-64.
#[tokio::test]
async fn distop_entity_bc_source_indicates_returned_version() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:bc-returned-version");
    let source_body = json!([vehicle(&entity_id, 42)]).to_string();
    mock.set_stub_with_raw_body(Method::GET, FORWARDED_ENTITIES, 200, &source_body, "application/ld+json;version=1.3")
        .await;

    let endpoint = mock.url("");
    register_inclusive(&topology, &endpoint, "registration:bc-returned-version", Some("1.3")).await;

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .query_entities(&format!("type=Vehicle&id={entity_id}"), "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let body = json_body(&response);
    assert!(
        find_entity(&body, &entity_id).is_some(),
        "the source entity is integrated with its indicated version: {body}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// When collating results the broker uses the source's version, so a 1.3
/// source's payload is not rejected for missing 1.9 members (clause 4.3.6.8).
/// Confirms REQ-FW-65.
#[tokio::test]
async fn distop_entity_bc_broker_uses_version_info_when_collating() {
    let topology = topology::single().await.expect("single-broker topology");
    let modern_mock = ContextSourceAdapter::start().await;
    let legacy_mock = ContextSourceAdapter::start().await;
    let modern_id = topology.id("Vehicle:bc-modern");
    let legacy_id = topology.id("Vehicle:bc-legacy");
    modern_mock
        .set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([vehicle(&modern_id, 60)]))
        .await;
    // A sparse 1.3 entity omitting newer members must still be integrated.
    let legacy_entity = json!([{ "id": legacy_id, "type": "Vehicle", "speed": { "type": "Property", "value": 20 } }]);
    legacy_mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &legacy_entity).await;

    let modern_endpoint = modern_mock.url("");
    let legacy_endpoint = legacy_mock.url("");
    register_inclusive(&topology, &modern_endpoint, "registration:bc-modern", None).await;
    register_inclusive(&topology, &legacy_endpoint, "registration:bc-legacy", Some("1.3")).await;

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .query_entities("type=Vehicle", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let body = json_body(&response);
    assert!(
        find_entity(&body, &legacy_id).is_some(),
        "the legacy source's sparse payload is not rejected: {body}"
    );

    modern_mock.shutdown().await;
    legacy_mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
