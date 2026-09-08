//! `contextSourceInfo` forwarding of headers, the `Accept`, and the
//! `Content-Type` of a forwarded request (ETSI GS CIM 009 v1.9.1, clause
//! 4.3.6.6). All `T-MOCK` for byte-level oracles, except the invalid-accept
//! registration row (`T-LOCAL`) and the tenant row (`T-TENANT`).

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    oracle::{assert_problem_details, assert_status, forwarded_header},
    reg::PEER_ENDPOINT,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_client::tenant_name::TenantName;
use andromeda_test_fixtures::problem_detail::BAD_REQUEST_DATA;
use axum::http::Method;
use serde_json::json;
use std::time::Duration;

/// Budget for a forwarded request to reach the mock context source.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// The path a broker appends to a registration `endpoint` for an entity query
/// or create.
const FORWARDED_ENTITIES: &str = "/ngsi-ld/v1/entities";

/// A static `contextSourceInfo` header is conveyed on the forwarded request
/// (clause 4.3.6.6). Confirms REQ-FW-33.
#[tokio::test]
async fn distop_entity_csi_static_header_conveyed_on_forward() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:csi-static"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["queryEntity"])
        .context_source_info("X-Auth", "abcd")
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
        .query_entities("type=Vehicle", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(
        forwarded_header(&forwarded, "x-auth").as_deref(),
        Some("abcd"),
        "the static header is conveyed on forward"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `contextSourceInfo` value of `urn:ngsi-ld:request` copies the header from
/// the triggering request onto the forwarded one (clause 4.3.6.6). Confirms
/// REQ-FW-34.
#[tokio::test]
async fn distop_entity_csi_urn_request_copies_from_triggering_request() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:csi-urn"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["queryEntity"])
        .context_source_info("user", "urn:ngsi-ld:request")
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
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle")
        .header("user", "abcd")
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("query with user header");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(
        forwarded_header(&forwarded, "user").as_deref(),
        Some("abcd"),
        "the triggering request's header is copied onto the forward"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The tenant is conveyed only via `NGSILD-Tenant`, never inside
/// `contextSourceInfo` (clause 4.3.6.6). Confirms REQ-FW-35.
#[tokio::test]
async fn distop_entity_csi_tenant_never_placed_inside_context_source_info() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let tenant = TenantName::parse("distopTenantCsi").expect("valid tenant name");
    let tenant_client = topology.broker(BrokerRole::A).tenant_client(tenant);
    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:csi-tenant"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["queryEntity"])
        .build();
    assert_status(&tenant_client.create_registration(&registration).await.expect("create registration"), 201);

    let response = tenant_client.query_entities("type=Vehicle", "application/ld+json", None).await.expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_eq!(
        forwarded_header(&forwarded, "ngsild-tenant").as_deref(),
        Some("distopTenantCsi"),
        "the tenant is carried in the NGSILD-Tenant header"
    );

    let registration_id = topology.id("registration:csi-tenant");
    assert_status(&tenant_client.delete_registration(&registration_id).await.expect("tenant cleanup"), 204);
    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `contextSourceInfo` key of `Content-length` cannot override the real body
/// length of the forwarded request (clause 4.3.6.6). Confirms REQ-FW-36.
#[tokio::test]
async fn distop_entity_csi_cannot_override_content_length() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:csi-content-length");
    mock.set_stub_with_body(Method::POST, FORWARDED_ENTITIES, 201, &json!({})).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:csi-content-length"), &endpoint)
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["createEntity"])
        .context_source_info("Content-Length", "9999")
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
        .post_entity(&vehicle(&entity_id, 30))
        .with_ld_json()
        .send()
        .await
        .expect("provision");
    assert!(created.status.is_success(), "the provision succeeds: {}", created.status);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    assert_ne!(
        forwarded_header(&forwarded, "content-length").as_deref(),
        Some("9999"),
        "the real body length overrides the contextSourceInfo Content-Length"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `contextSourceInfo` `accept` of `application/ld+json` sets the forwarded
/// `Accept` (clause 4.3.6.6). Confirms REQ-FW-37.
#[tokio::test]
async fn distop_entity_csi_accept_ld_json_sets_forwarded_accept() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:csi-accept-ld"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["queryEntity"])
        .context_source_info("accept", "application/ld+json")
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
        .query_entities("type=Vehicle", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let accept = forwarded_header(&forwarded, "accept").unwrap_or_default();
    assert!(accept.contains("application/ld+json"), "the forwarded Accept is application/ld+json: {accept}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `contextSourceInfo` `accept` of `application/json` sets the forwarded
/// `Accept` (clause 4.3.6.6). Confirms REQ-FW-37.
#[tokio::test]
async fn distop_entity_csi_accept_plain_json_sets_forwarded_accept() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES, 200, &json!([])).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:csi-accept-json"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
        .operations(&["queryEntity"])
        .context_source_info("accept", "application/json")
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
        .query_entities("type=Vehicle", "application/ld+json", None)
        .await
        .expect("query");
    assert_status(&response, 200);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let accept = forwarded_header(&forwarded, "accept").unwrap_or_default();
    assert!(accept.contains("application/json"), "the forwarded Accept is application/json: {accept}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `contextSourceInfo` `accept` of `text/plain` is rejected at registration
/// with 400 `BadRequestData` (clause 4.3.6.6, 5.2.9). Confirms REQ-FW-37.
#[tokio::test]
async fn distop_entity_csi_accept_invalid_rejected_at_registration() {
    let topology = topology::single().await.expect("single-broker topology");
    let registration = CsrBuilder::new(&topology.id("registration:csi-accept-invalid"), PEER_ENDPOINT)
        .mode("inclusive")
        .entity_type("Vehicle")
        .context_source_info("accept", "text/plain")
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

/// A source responding `application/json` is converted to the client's requested
/// `application/ld+json`, with an inline `@context` (clause 4.3.6.6). Confirms
/// REQ-FW-38.
#[tokio::test]
async fn distop_entity_csi_source_response_converted_to_client_type() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:csi-convert");
    let source_body = json!([{ "id": entity_id, "type": "Vehicle", "speed": { "type": "Property", "value": 42 } }]).to_string();
    mock.set_stub_with_raw_body(Method::GET, FORWARDED_ENTITIES, 200, &source_body, "application/json")
        .await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:csi-convert"), &endpoint)
        .mode("inclusive")
        .entity_type("Vehicle")
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
    let content_type = response.headers.get("content-type").and_then(|value| value.to_str().ok()).unwrap_or_default();
    assert!(
        content_type.contains("application/ld+json"),
        "the aggregated response is ld+json: {content_type}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `contextSourceInfo` `contentType` of `application/json` constrains the
/// forwarded `Content-Type` (clause 4.3.6.6). Confirms REQ-FW-39.
#[tokio::test]
async fn distop_entity_csi_content_type_json_constrains_forwarded_content_type() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:csi-ct-json");
    mock.set_stub_with_body(Method::POST, FORWARDED_ENTITIES, 201, &json!({})).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:csi-ct-json"), &endpoint)
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["createEntity"])
        .context_source_info("contentType", "application/json")
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
        "the forwarded Content-Type is application/json: {content_type}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `contextSourceInfo` `contentType` of `application/ld+json` constrains the
/// forwarded `Content-Type` and the body carries an inline `@context` (clause
/// 4.3.6.6). Confirms REQ-FW-39, REQ-FW-40.
#[tokio::test]
async fn distop_entity_csi_content_type_ld_json_constrains_forwarded_content_type() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:csi-ct-ld");
    mock.set_stub_with_body(Method::POST, FORWARDED_ENTITIES, 201, &json!({})).await;

    let endpoint = mock.url("");
    let registration = CsrBuilder::new(&topology.id("registration:csi-ct-ld"), &endpoint)
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["createEntity"])
        .context_source_info("contentType", "application/ld+json")
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
        .post_entity(&vehicle(&entity_id, 30))
        .with_ld_json()
        .send()
        .await
        .expect("provision");
    assert!(created.status.is_success(), "the provision succeeds: {}", created.status);

    let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
    let content_type = forwarded_header(&forwarded, "content-type").unwrap_or_default();
    assert!(
        content_type.contains("application/ld+json"),
        "the forwarded Content-Type is ld+json: {content_type}"
    );
    let forwarded_body = json_body_from_bytes(&forwarded.body);
    assert!(
        forwarded_body.get("@context").is_some(),
        "the ld+json body carries an inline @context: {forwarded_body}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Decode a captured request body as JSON, failing with the raw bytes on error.
fn json_body_from_bytes(body: &[u8]) -> serde_json::Value {
    match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(error) => panic!("forwarded body is not JSON ({error}); raw: {}", String::from_utf8_lossy(body)),
    }
}
