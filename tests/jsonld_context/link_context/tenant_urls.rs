//! A wrapper linked for a non-default tenant is dereferenceable by anyone who
//! follows the link, and visible to no other tenant (study B14).
//!
//! Clause 4.14 keeps a tenant's information to that tenant, and a JSON-LD
//! processor dereferencing a link sends no `NGSILD-Tenant`, so the URL names its
//! tenant with `andromedaTenant`.

use crate::{
    context_api::{dereference, extract_id, implicit_context_ids},
    context_links::single_context_link,
    core_contexts::core,
    ngsi_requests::{create_device, get},
    private_tenant::{PrivateTenant, tenant_client},
    problem_types::BAD_REQUEST_DATA_TYPE,
    unique_ids::unique_id,
    user_contexts::UserContexts,
};
use reqwest::{Client, StatusCode};
use serde_json::json;

/// The wrapper URL names its tenant, is dereferenced with no header and with
/// the agreeing header, refuses a disagreeing header, and is listed by its own
/// tenant only.
#[tokio::test]
async fn a_non_default_tenants_wrapper_is_dereferenceable_and_private_to_it() {
    let tenant = PrivateTenant::provisioned().await;
    let other = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:tenant-url");
    create_device(tenant.client(), &device, &contexts.terms).await;
    let response = get(tenant.client(), &format!("entities/{device}"), "application/json", Some(&contexts.terms)).await;
    let wrapper = single_context_link(response.headers());
    assert!(wrapper.ends_with(&format!("?andromedaTenant={}", tenant.name())), "{wrapper}");
    let document = json!({ "@context": [contexts.terms, core()] });

    let headerless = dereference(&Client::new(), &wrapper).await;
    assert_eq!(headerless.status, StatusCode::OK);
    assert_eq!(headerless.json(), document);
    let agreeing = dereference(tenant.client(), &wrapper).await;
    assert_eq!(agreeing.status, StatusCode::OK);
    assert_eq!(agreeing.json(), document);
    let disagreeing = dereference(&tenant_client(other.name()), &wrapper).await;
    assert_eq!(disagreeing.status, StatusCode::BAD_REQUEST);
    assert_eq!(disagreeing.json()["type"], BAD_REQUEST_DATA_TYPE.as_str());

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
