//! A wrapper linked for a non-default tenant is dereferenceable under that
//! tenant's header, and visible to no other tenant.
//!
//! Clause 4.14 keeps a tenant's information to that tenant, and clauses 4.14
//! and 6.3.14 make the `NGSILD-Tenant` header the only way a request names a
//! tenant, so the URL names no tenant and is dereferenced with the header.

use crate::{
    context_api::{dereference, extract_id, implicit_context_ids},
    context_links::single_context_link,
    core_contexts::core,
    ngsi_requests::{create_device, get},
    private_tenant::{PrivateTenant, tenant_client},
    unique_ids::unique_id,
    user_contexts::UserContexts,
};
use reqwest::{Client, StatusCode};
use serde_json::json;

/// The wrapper URL names no tenant, is dereferenced under its tenant's header,
/// is not found without a header or under another tenant's, and is listed by
/// its own tenant only.
#[tokio::test]
async fn a_non_default_tenants_wrapper_is_dereferenceable_and_private_to_it() {
    let tenant = PrivateTenant::provisioned().await;
    let other = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:tenant-url");
    create_device(tenant.client(), &device, &contexts.terms).await;
    let response = get(tenant.client(), &format!("entities/{device}"), "application/json", Some(&contexts.terms)).await;
    let wrapper = single_context_link(response.headers());
    assert!(!wrapper.contains('?'), "the URL names no tenant: {wrapper}");
    let document = json!({ "@context": [contexts.terms, core()] });

    let under_header = dereference(tenant.client(), &wrapper).await;
    assert_eq!(under_header.status, StatusCode::OK);
    assert_eq!(under_header.json(), document);
    let headerless = dereference(&Client::new(), &wrapper).await;
    assert_eq!(headerless.status, StatusCode::NOT_FOUND, "the default tenant does not hold the wrapper");
    let other_header = dereference(&tenant_client(other.name()), &wrapper).await;
    assert_eq!(other_header.status, StatusCode::NOT_FOUND, "another tenant does not hold the wrapper");

    assert_eq!(implicit_context_ids(tenant.client()).await, vec![extract_id(&wrapper)]);
    assert!(implicit_context_ids(other.client()).await.is_empty(), "another tenant does not see the wrapper");
    assert!(
        !implicit_context_ids(&Client::new()).await.contains(&extract_id(&wrapper)),
        "the default tenant does not see the wrapper"
    );
    contexts.shutdown().await;
    tenant.finish().await;
    other.finish().await;
}
