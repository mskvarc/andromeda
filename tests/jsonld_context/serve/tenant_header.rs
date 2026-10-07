//! Serving a non-default tenant's document (clauses 4.14 and 6.3.14).
//!
//! The `NGSILD-Tenant` header is the only way a request names a tenant, Serve
//! @context included: a request carrying the header reaches that tenant's
//! document, a request without it reads the default tenant, and the
//! vendor tenant parameter is an undefined parameter (clause 6.3.20).

use crate::{
    broker_endpoints::broker_url,
    context_api::{add_hosted, get_context},
    private_tenant::{PrivateTenant, tenant_client},
    problem_types::{INVALID_REQUEST_TYPE, NONEXISTENT_TENANT_TYPE},
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};

/// The document every case below stores.
fn document() -> Value {
    json!({ "@context": { "Spot": "https://e.org/Spot" } })
}

/// The header names the tenant whose document is served, in full and as its
/// metadata projection.
#[tokio::test]
async fn a_non_default_tenants_document_is_served_under_its_header() {
    let tenant = PrivateTenant::provisioned().await;
    let id = add_hosted(tenant.client(), &document()).await;

    let (status, body) = get_context(tenant.client(), &id, "").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, document());

    let (status, details) = get_context(tenant.client(), &id, "details=true").await;
    assert_eq!(status, StatusCode::OK, "{details}");
    assert_eq!(details["kind"], "Hosted");
    tenant.finish().await;
}

/// Without the header the default tenant is read (clause 6.3.14), and it does
/// not hold another tenant's document.
#[tokio::test]
async fn the_document_is_not_served_to_the_default_tenant() {
    let tenant = PrivateTenant::provisioned().await;
    let id = add_hosted(tenant.client(), &document()).await;

    let (status, body) = get_context(&Client::new(), &id, "").await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the default tenant does not hold another tenant's document: {body}"
    );
    tenant.finish().await;
}

/// Clause 6.3.20: the vendor tenant parameter is undefined on every
/// operation, Serve included, so it is `InvalidRequest` and changes nothing.
#[tokio::test]
async fn the_former_tenant_parameter_is_an_undefined_parameter() {
    let tenant = PrivateTenant::provisioned().await;
    let id = add_hosted(tenant.client(), &document()).await;
    let parameter = format!("andromedaTenant={}", tenant.name());
    let encoded = urlencoding::encode(&id);

    let headerless_serve = Client::new()
        .get(broker_url(&format!("jsonldContexts/{encoded}?{parameter}")))
        .send()
        .await
        .expect("serve request failed");
    let serve = tenant
        .client()
        .get(broker_url(&format!("jsonldContexts/{encoded}?{parameter}")))
        .send()
        .await
        .expect("serve request failed");
    let listed = tenant
        .client()
        .get(broker_url(&format!("jsonldContexts?{parameter}")))
        .send()
        .await
        .expect("list request failed");
    let deleted = tenant
        .client()
        .delete(broker_url(&format!("jsonldContexts/{encoded}?{parameter}")))
        .send()
        .await
        .expect("delete request failed");

    for response in [headerless_serve, serve, listed, deleted] {
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let problem: Value = response.json().await.expect("a problem detail");
        assert_eq!(problem["type"], INVALID_REQUEST_TYPE.as_str());
    }
    let (still_there, _) = get_context(tenant.client(), &id, "").await;
    assert_eq!(still_there, StatusCode::OK, "the refused delete removed nothing");
    tenant.finish().await;
}

/// Clause 5.5.10: a header naming a tenant that does not exist is
/// `NonexistentTenant`.
#[tokio::test]
async fn a_header_naming_a_tenant_that_does_not_exist_is_nonexistent_tenant() {
    let (status, problem) = get_context(&tenant_client("no_such_tenant_ever"), "urn:ngsi-ld:JsonLdContext:any", "").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(problem["type"], NONEXISTENT_TENANT_TYPE.as_str());
}
