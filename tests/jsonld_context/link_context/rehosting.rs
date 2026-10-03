//! A deleted wrapper is hosted again under the same identifier (study B15).
//!
//! Clause 5.13.5 lets a client delete an `ImplicitlyCreated` @context. Its
//! identifier is derived from its content, so the next response that needs it
//! stores it again under the same identifier: in the process that deleted it at
//! once, and in any other at the latest one revalidation interval later.

use crate::{
    broker_endpoints::second_broker_url,
    context_api::{delete_context, dereference, extract_id},
    context_links::{context_link, single_context_link},
    core_contexts::core,
    ngsi_requests::{create_device, get},
    polling::eventually,
    private_tenant::PrivateTenant,
    unique_ids::unique_id,
    user_contexts::UserContexts,
};
use reqwest::{Client, StatusCode};
use serde_json::json;

/// The wrapper the second broker links for `path` under `link`.
async fn linked_by_second_broker(client: &Client, path: &str, link: &str) -> String {
    let response = client
        .get(second_broker_url(path))
        .header("Accept", "application/json")
        .header("Link", context_link(link))
        .send()
        .await
        .expect("retrieval on the second broker failed");
    assert_eq!(response.status(), StatusCode::OK);
    single_context_link(response.headers())
}

/// Both brokers link the same content-derived wrapper; deleted, it is hosted
/// again at once by the deleting process, and by the other process within one
/// revalidation interval, under the same identifier.
#[tokio::test]
async fn a_deleted_wrapper_is_hosted_again_under_the_same_identifier() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:rehosted");
    create_device(tenant.client(), &device, &contexts.terms).await;
    let path = format!("entities/{device}");
    let document = json!({ "@context": [contexts.terms, core()] });
    let wrapper = single_context_link(get(tenant.client(), &path, "application/json", Some(&contexts.terms)).await.headers());
    let identifier = extract_id(&wrapper);
    assert_eq!(
        extract_id(&linked_by_second_broker(tenant.client(), &path, &contexts.terms).await),
        identifier,
        "equal content is one wrapper for every process"
    );

    let (deleted, _) = delete_context(tenant.client(), &identifier, "").await;
    assert_eq!(deleted, StatusCode::NO_CONTENT);
    assert_eq!(dereference(tenant.client(), &wrapper).await.status, StatusCode::NOT_FOUND);
    let again = single_context_link(get(tenant.client(), &path, "application/json", Some(&contexts.terms)).await.headers());
    assert_eq!(again, wrapper, "the deleting process hosts it again at once, under the same identifier");
    assert_eq!(dereference(tenant.client(), &again).await.json(), document);

    let (deleted_again, _) = delete_context(tenant.client(), &identifier, "").await;
    assert_eq!(deleted_again, StatusCode::NO_CONTENT);
    let client = tenant.client();
    let terms = &contexts.terms;
    let path = &path;
    let rehosted = eventually("the other process hosts the deleted wrapper again", move || async move {
        let linked = linked_by_second_broker(client, path, terms).await;
        let served = dereference(client, &linked).await;
        (served.status == StatusCode::OK).then_some((linked, served.json()))
    })
    .await;
    assert_eq!(extract_id(&rehosted.0), identifier, "under the same identifier");
    assert_eq!(rehosted.1, document);
    contexts.shutdown().await;
    tenant.finish().await;
}
