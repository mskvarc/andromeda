//! Context Source identity retrieval (clause 5.15.1): the `ContextSourceIdentity`
//! body, per-tenant alias, parameterless input, and the JSON-LD content
//! negotiation. Local rows are `T-LOCAL`; the per-tenant row is `T-TENANT`.

use crate::{
    oracle::{assert_status, json_body},
    topology::{self, BrokerRole},
};
use andromeda_test_client::tenant_name::TenantName;
use axum::http::Method;
use pretty_assertions::assert_eq;

/// Retrieving the source identity returns a `ContextSourceIdentity` (clause 5.15.1).
/// Confirms REQ-MAP-157.
#[tokio::test]
async fn identity_retrieve_returns_context_source_identity() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .source_identity(Some("application/ld+json"))
        .await
        .expect("source identity");
    assert_status(&response, 200);
    assert_eq!(
        json_body(&response)["type"].as_str(),
        Some("ContextSourceIdentity"),
        "the body is a ContextSourceIdentity"
    );
    topology.finish().await.expect("topology cleanup");
}

/// A tenant obtains a tenant-specific identity (clause 5.15.1, 4.14).
/// Confirms REQ-MAP-158.
#[tokio::test]
async fn identity_per_tenant_alias_obtainable() {
    let topology = topology::pair().await.expect("two-broker topology");
    let tenant = TenantName::parse("distopTenantMap").expect("valid tenant name");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .tenant_client(tenant)
        .source_identity(Some("application/ld+json"))
        .await
        .expect("tenant source identity");
    assert_status(&response, 200);
    assert!(
        !json_body(&response)["contextSourceAlias"].as_str().unwrap_or_default().is_empty(),
        "the tenant identity carries a non-empty alias"
    );
    topology.finish().await.expect("topology cleanup");
}

/// The identity endpoint takes no body or parameters, so extra query parameters
/// are ignored (clause 5.15.1). Confirms REQ-MAP-159.
#[tokio::test]
async fn identity_input_takes_no_body_or_params() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/info/sourceIdentity?ignored=value")
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("source identity with ignored params");
    assert_status(&response, 200);
    assert_eq!(
        json_body(&response)["type"].as_str(),
        Some("ContextSourceIdentity"),
        "extra parameters are ignored"
    );
    topology.finish().await.expect("topology cleanup");
}

/// A source that does not support identity answers `501 Not Implemented`
/// (clause 5.15.1). Tracked gap: a conformant Andromeda always supplies its
/// identity.
#[ignore = "GAP REQ-MAP-160: source-identity 501 unsupported branch not implemented"]
#[tokio::test]
async fn identity_unsupported_returns_501() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .source_identity(Some("application/ld+json"))
        .await
        .expect("source identity");
    assert_status(&response, 501);
    topology.finish().await.expect("topology cleanup");
}

/// The identity body conforms to the `ContextSourceIdentity` data model
/// (clause 5.2.40). Confirms REQ-MAP-161.
#[tokio::test]
async fn identity_body_conforms_to_data_model() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .source_identity(Some("application/ld+json"))
        .await
        .expect("source identity");
    assert_status(&response, 200);
    let body = json_body(&response);
    assert!(body["id"].as_str().is_some(), "the identity carries an id: {body}");
    assert_eq!(
        body["type"].as_str(),
        Some("ContextSourceIdentity"),
        "the identity type is ContextSourceIdentity"
    );
    assert!(
        body["contextSourceAlias"].as_str().is_some(),
        "the identity carries a contextSourceAlias: {body}"
    );
    topology.finish().await.expect("topology cleanup");
}

/// Success returns `200` as `application/ld+json` with the context embedded
/// (clause 5.15.1, 5.5.7). Confirms REQ-MAP-162.
#[tokio::test]
async fn identity_success_returns_200_jsonld() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .source_identity(Some("application/ld+json"))
        .await
        .expect("source identity");
    assert_status(&response, 200);
    let content_type = response.headers.get("content-type").and_then(|value| value.to_str().ok()).unwrap_or_default();
    assert!(
        content_type.contains("application/ld+json"),
        "the ld+json response has an ld+json content type: {content_type}"
    );
    assert!(json_body(&response).get("@context").is_some(), "the ld+json body embeds the context");
    topology.finish().await.expect("topology cleanup");
}
