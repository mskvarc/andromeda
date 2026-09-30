//! Serving a non-default tenant's document to a caller that sends no tenant
//! header (study B14, Serve side).
//!
//! A JSON-LD processor dereferencing a context URL sends no `NGSILD-Tenant`, so
//! every URL the broker mints for a non-default tenant's stored `@context`
//! carries `andromedaTenant` (`docs/extensions/tenant-qualified-context-urls.md`).
//! Serve reads the tenant from the header, else the parameter, else the default
//! tenant (clause 6.3.14 omits the header for it); every other operation
//! refuses the parameter as one it does not define (clause 6.3.20).

use crate::{
    broker_endpoints::broker_url,
    context_api::{add_hosted, get_context},
    private_tenant::PrivateTenant,
    problem_types::{BAD_REQUEST_DATA_TYPE, INVALID_REQUEST_TYPE, NONEXISTENT_TENANT_TYPE},
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};

/// Serve `id` with `query` through `client`: the status and the body.
async fn serve_with(client: &Client, id: &str, query: &str) -> (StatusCode, Value) {
    let response = client
        .get(broker_url(&format!("jsonldContexts/{}?{query}", urlencoding::encode(id))))
        .send()
        .await
        .expect("serve request failed");
    let status = response.status();
    (status, response.json().await.unwrap_or_default())
}

/// The parameter alone reaches the tenant's document; with the header naming
/// the same tenant it agrees; the default tenant does not hold the document.
#[tokio::test]
async fn the_andromeda_tenant_parameter_reaches_a_non_default_tenants_document_without_a_header() {
    let tenant = PrivateTenant::provisioned().await;
    let id = add_hosted(tenant.client(), &json!({ "@context": { "Spot": "https://e.org/Spot" } })).await;
    let headerless = Client::new();
    let parameter = format!("andromedaTenant={}", tenant.name());

    let (status, body) = serve_with(&headerless, &id, &parameter).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "@context": { "Spot": "https://e.org/Spot" } }));

    let (agreeing, _) = serve_with(tenant.client(), &id, &parameter).await;
    assert_eq!(agreeing, StatusCode::OK);

    let (default_tenant, _) = get_context(&headerless, &id, "").await;
    assert_eq!(
        default_tenant,
        StatusCode::NOT_FOUND,
        "the default tenant does not hold another tenant's document"
    );
    tenant.finish().await;
}

/// The metadata projection is reached the same way.
#[tokio::test]
async fn the_andromeda_tenant_parameter_reaches_the_details_projection_too() {
    let tenant = PrivateTenant::provisioned().await;
    let id = add_hosted(tenant.client(), &json!({ "@context": { "Spot": "https://e.org/Spot" } })).await;

    let (status, body) = serve_with(&Client::new(), &id, &format!("details=true&andromedaTenant={}", tenant.name())).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kind"], "Hosted");
    tenant.finish().await;
}

/// A header and a parameter naming different tenants is `BadRequestData`.
#[tokio::test]
async fn a_header_and_a_parameter_naming_different_tenants_is_bad_request_data() {
    let tenant = PrivateTenant::provisioned().await;
    let id = add_hosted(tenant.client(), &json!({ "@context": { "Spot": "https://e.org/Spot" } })).await;

    let (status, problem) = serve_with(tenant.client(), &id, "andromedaTenant=someone_else").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(problem["type"], BAD_REQUEST_DATA_TYPE.as_str());
    tenant.finish().await;
}

/// Clause 5.5.10: a tenant that does not exist is `NonexistentTenant`.
#[tokio::test]
async fn a_parameter_naming_a_tenant_that_does_not_exist_is_nonexistent_tenant() {
    let (status, problem) = serve_with(&Client::new(), "urn:ngsi-ld:JsonLdContext:any", "andromedaTenant=no_such_tenant_ever").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(problem["type"], NONEXISTENT_TENANT_TYPE.as_str());
}

/// Clause 6.3.20: on every other operation the parameter is undefined, which
/// is `InvalidRequest`.
#[tokio::test]
async fn the_parameter_on_any_other_operation_is_invalid_request() {
    let tenant = PrivateTenant::provisioned().await;
    let id = add_hosted(tenant.client(), &json!({ "@context": { "Spot": "https://e.org/Spot" } })).await;
    let parameter = format!("andromedaTenant={}", tenant.name());

    let listed = tenant
        .client()
        .get(broker_url(&format!("jsonldContexts?{parameter}")))
        .send()
        .await
        .expect("list request failed");
    let deleted = tenant
        .client()
        .delete(broker_url(&format!("jsonldContexts/{}?{parameter}", urlencoding::encode(&id))))
        .send()
        .await
        .expect("delete request failed");
    let queried = tenant
        .client()
        .get(broker_url(&format!("entities?type=Spot&{parameter}")))
        .send()
        .await
        .expect("query request failed");

    for response in [listed, deleted, queried] {
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let problem: Value = response.json().await.expect("a problem detail");
        assert_eq!(problem["type"], INVALID_REQUEST_TYPE.as_str());
    }
    let (still_there, _) = get_context(tenant.client(), &id, "").await;
    assert_eq!(still_there, StatusCode::OK, "the refused delete removed nothing");
    tenant.finish().await;
}
