//! `contextSourceInfo` JSON-LD context handling and NGSI-LD conformance version
//! negotiation on forwarded requests (ETSI GS CIM 009 v1.9.1, clauses 4.3.6.6,
//! 4.3.6.8). All `T-MOCK` for byte-level oracles, except registration-validation
//! rows (`T-LOCAL`).

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    oracle::{assert_problem_details, assert_status, forwarded_header},
    reg::PEER_ENDPOINT,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::{context::COMPOUND_CONTEXT, problem_detail::BAD_REQUEST_DATA};
use axum::http::Method;
use serde_json::{Value, json};
use std::time::Duration;

/// Budget for a forwarded request to reach the mock context source.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// The path a broker appends to a registration `endpoint` for an entity query
/// or create.
const FORWARDED_ENTITIES: &str = "/ngsi-ld/v1/entities";

/// A distinctive fragment of the compound test-suite context URL, used to prove
/// that context governs a forwarded request's `Link`.
const COMPOUND_CONTEXT_MARKER: &str = "ngsi-ld-test-suite-compound";

/// With a JSON `contentType`, the `@context` is provided as a `Link` header
/// rather than inline, matching the forwarded MIME type (clause 4.3.6.6).
/// Confirms REQ-FW-40.
#[tokio::test]
async fn distop_entity_csi_context_provided_per_mime_requirement() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:csi-mime-context");
    mock.set_stub_with_body(Method::POST, FORWARDED_ENTITIES, 201, &json!({})).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:csi-mime-context"), &endpoint)
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["createEntity"])
        .context_source_info("contentType", "application/json")
        .context_source_info("jsonldContext", COMPOUND_CONTEXT.as_str())
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

    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .post_entity(&vehicle(&entity_id, 30))
        .with_ld_json()
        .send()
        .await
        .expect("provision");
    assert!(created.status.is_success(), "the provision succeeds: {}", created.status);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let link = forwarded_header(&forwarded, "link").unwrap_or_default();
    assert!(
        link.contains(COMPOUND_CONTEXT_MARKER),
        "the JSON MIME conveys the context via a Link header: {link}"
    );
    let body: Value = forwarded.json().expect("forwarded body is JSON");
    assert!(body.get("@context").is_none(), "the JSON MIME body carries no inline @context: {body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `jsonldContext` value must be a valid URL: a malformed value is rejected at
/// registration with 400, a valid one is accepted (clause 4.3.6.6, 5.2.9).
/// Confirms REQ-FW-41.
#[tokio::test]
async fn distop_entity_csi_jsonld_context_must_be_url_validated() {
    let topology = topology::single().await.expect("single-broker topology");

    let malformed = CsrBuilder::new(&topology.id("registration:csi-context-malformed"), PEER_ENDPOINT)
        .mode("inclusive")
        .entity_type("Vehicle")
        .context_source_info("jsonldContext", "### not a url ###")
        .build();
    let rejected = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_registration(&malformed)
        .await
        .expect("malformed registration");
    assert_status(&rejected, 400);
    assert_problem_details(&rejected, BAD_REQUEST_DATA);

    let valid = CsrBuilder::new(&topology.id("registration:csi-context-valid"), PEER_ENDPOINT)
        .mode("inclusive")
        .entity_type("Vehicle")
        .context_source_info("jsonldContext", COMPOUND_CONTEXT.as_str())
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&valid)
            .await
            .expect("valid registration"),
        201,
    );

    topology.finish().await.expect("topology cleanup");
}

/// A custom `jsonldContext` governs the forwarded request: it is carried as the
/// context that compacts the forwarded payload and parameters (clause 4.3.6.6).
/// Confirms REQ-FW-42.
#[tokio::test]
async fn distop_entity_csi_jsonld_context_compacts_payload_and_params() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:csi-context-compact"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["queryEntity"])
        .context_source_info("jsonldContext", COMPOUND_CONTEXT.as_str())
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
    let link = forwarded_header(&forwarded, "link").unwrap_or_default();
    assert!(
        link.contains(COMPOUND_CONTEXT_MARKER),
        "the custom context governs the forwarded request: {link}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `jsonldContext` with a payload forwards `application/json`, strips the
/// inline `@context`, and conveys the context via a `Link` header (clause
/// 4.3.6.6). Confirms REQ-FW-43.
#[tokio::test]
async fn distop_entity_csi_jsonld_context_with_payload_sets_json_and_strips_context() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:csi-context-payload");
    mock.set_stub_with_body(Method::POST, FORWARDED_ENTITIES, 201, &json!({})).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:csi-context-payload"), &endpoint)
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["createEntity"])
        .context_source_info("jsonldContext", COMPOUND_CONTEXT.as_str())
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

    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .post_entity(&vehicle(&entity_id, 30))
        .with_ld_json()
        .send()
        .await
        .expect("provision");
    assert!(created.status.is_success(), "the provision succeeds: {}", created.status);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let content_type = forwarded_header(&forwarded, "content-type").unwrap_or_default();
    assert!(
        content_type.contains("application/json"),
        "the payload is forwarded as application/json: {content_type}"
    );
    let link = forwarded_header(&forwarded, "link").unwrap_or_default();
    assert!(link.contains(COMPOUND_CONTEXT_MARKER), "the context is conveyed via a Link header: {link}");
    let body: Value = forwarded.json().expect("forwarded body is JSON");
    assert!(body.get("@context").is_none(), "the inline @context is stripped from the JSON body: {body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The NGSI-LD conformance value form is major.minor: `1.5` is accepted while
/// `1` and `1.5.0` are rejected at registration (clause 4.3.6.8). Confirms
/// REQ-FW-44.
#[tokio::test]
async fn distop_entity_csi_ngsild_conformance_value_form_major_minor() {
    let topology = topology::single().await.expect("single-broker topology");
    let accepted = CsrBuilder::new(&topology.id("registration:csi-conformance-ok"), PEER_ENDPOINT)
        .mode("inclusive")
        .entity_type("Vehicle")
        .context_source_info("ngsildConformance", "1.5")
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&accepted)
            .await
            .expect("valid conformance"),
        201,
    );

    for bad in ["1", "1.5.0"] {
        let registration = CsrBuilder::new(&topology.id(&format!("registration:csi-conformance-{bad}")), PEER_ENDPOINT)
            .mode("inclusive")
            .entity_type("Vehicle")
            .context_source_info("ngsildConformance", bad)
            .build();
        let rejected = topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("invalid conformance");
        assert_status(&rejected, 400);
    }

    topology.finish().await.expect("topology cleanup");
}

/// A registration declaring a `1.3` source conformance triggers the clause
/// 4.3.6.8 backwards-compatible transform on the forwarded payload, so a `scope`
/// is removed (clause 4.3.6.8). Confirms REQ-FW-45.
#[tokio::test]
async fn distop_entity_csi_ngsild_conformance_triggers_backwards_compat_transform() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:csi-conformance-transform");
    mock.set_stub_with_body(Method::POST, FORWARDED_ENTITIES, 201, &json!({})).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:csi-conformance-transform"), &endpoint)
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["createEntity"])
        .context_source_info("ngsildConformance", "1.3")
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

    let mut body = vehicle(&entity_id, 30);
    body["scope"] = json!("/Madrid");
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .post_entity(&body)
        .with_ld_json()
        .send()
        .await
        .expect("provision");
    assert!(created.status.is_success(), "the provision succeeds: {}", created.status);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let forwarded_body: Value = forwarded.json().expect("forwarded body is JSON");
    assert!(
        forwarded_body.get("scope").is_none(),
        "the 4.3.6.8 downgrade removes scope for a 1.3 source: {forwarded_body}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
