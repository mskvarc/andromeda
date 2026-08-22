//! `contextSourceInfo`-derived request headers on forwarded requests (clause
//! 6.3.19 / 4.3.6.5): a `jsonldContext` entry sets the JSON-LD `Link` and JSON
//! content type, other keys become custom headers, the `urn:ngsi-ld:request`
//! token copies a value from the triggering request, and neither `NGSILD-Tenant`
//! nor `Content-Length` can be overridden. All `T-MOCK`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    http::forwarding::{FORWARDED_ENTITIES_PATH, context_link, context_link_header},
    oracle::{assert_status, forwarded_header},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use reqwest::Method;
use serde_json::json;
use std::time::Duration;

/// A `jsonldContext` contextSourceInfo entry makes the forwarded request carry a
/// JSON-LD `Link` header and a JSON content type (clause 6.3.19). Confirms
/// REQ-HTTP-40.
#[tokio::test]
async fn dist_http_forwarded_jsonld_context_sets_link_and_json_content_type() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:csi-jsonld");
    let reg_id = topology.id("registration:csi-jsonld");
    let mock = ContextSourceAdapter::start().await;

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("exclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["createEntity"])
        .context_source_info("jsonldContext", context_link())
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&registration)
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::POST, FORWARDED_ENTITIES_PATH, 201).await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&vehicle(&entity_id, 9))
            .with_ld_json()
            .send()
            .await
            .expect("forwarded create"),
        201,
    );
    let forwarded = mock.next_request(Duration::from_secs(5)).await;
    assert!(
        forwarded_header(&forwarded, "link").is_some(),
        "the forwarded request carries a JSON-LD Link header"
    );
    let content_type = forwarded_header(&forwarded, "content-type").unwrap_or_default();
    assert!(
        content_type.contains("json"),
        "the forwarded request carries a JSON content type, got {content_type}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A non-reserved contextSourceInfo key becomes a custom header on the forwarded
/// request (clause 6.3.19). Confirms REQ-HTTP-41.
#[tokio::test]
async fn dist_http_forwarded_context_source_info_keys_become_custom_headers() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:csi-custom");
    let reg_id = topology.id("registration:csi-custom");
    let mock = ContextSourceAdapter::start().await;

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["retrieveEntity", "queryEntity", "queryBatch"])
        .context_source_info("X-Custom-Header", "custom-value")
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&registration)
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .query_entities("type=Vehicle", "application/ld+json", Some(context_link()))
            .await
            .expect("forwarded query"),
        200,
    );
    let forwarded = mock.next_request(Duration::from_secs(5)).await;
    assert_eq!(
        forwarded_header(&forwarded, "x-custom-header").as_deref(),
        Some("custom-value"),
        "the contextSourceInfo key becomes a custom header"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `urn:ngsi-ld:request` contextSourceInfo value copies the same-named header
/// from the triggering request onto the forwarded request (clause 4.3.6.5).
/// Confirms REQ-HTTP-42.
#[tokio::test]
async fn dist_http_forwarded_context_source_info_request_value_copied_from_request() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:csi-request");
    let reg_id = topology.id("registration:csi-request");
    let mock = ContextSourceAdapter::start().await;

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["retrieveEntity", "queryEntity", "queryBatch"])
        .context_source_info("X-User", "urn:ngsi-ld:request")
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&registration)
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle")
        .header("accept", "application/ld+json")
        .header("link", context_link_header())
        .header("X-User", "abcd")
        .send()
        .await
        .expect("query with copied header");
    assert_status(&response, 200);
    let forwarded = mock.next_request(Duration::from_secs(5)).await;
    assert_eq!(
        forwarded_header(&forwarded, "x-user").as_deref(),
        Some("abcd"),
        "urn:ngsi-ld:request copies the value from the triggering request"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A contextSourceInfo entry cannot override the tenant or the content length of
/// the forwarded request (clause 4.3.6.5). Confirms REQ-HTTP-43.
#[tokio::test]
async fn dist_http_forwarded_context_source_info_cannot_override_tenant_or_content_length() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:csi-protected");
    let reg_id = topology.id("registration:csi-protected");
    let mock = ContextSourceAdapter::start().await;

    let registration = CsrBuilder::new(&reg_id, &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .operations(&["retrieveEntity", "queryEntity", "queryBatch"])
        .context_source_info("NGSILD-Tenant", "evil-tenant")
        .context_source_info("Content-Length", "999999")
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_registration(&registration)
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .query_entities("type=Vehicle", "application/ld+json", Some(context_link()))
            .await
            .expect("forwarded query"),
        200,
    );
    let forwarded = mock.next_request(Duration::from_secs(5)).await;
    assert_ne!(
        forwarded_header(&forwarded, "ngsild-tenant").as_deref(),
        Some("evil-tenant"),
        "contextSourceInfo cannot override the tenant"
    );
    assert_ne!(
        forwarded_header(&forwarded, "content-length").as_deref(),
        Some("999999"),
        "contextSourceInfo cannot override the content length"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
