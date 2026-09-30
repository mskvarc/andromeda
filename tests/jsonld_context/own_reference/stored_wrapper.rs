//! The URL of a wrapper the broker minted, named back to it.

use crate::{
    broker_endpoints::{broker_url, second_broker_url},
    context_api::{CONTEXTS_PATH, cached_urls},
    context_links::{context_link, single_context_link},
    ngsi_requests::{create_device, get, stored_entity},
    private_tenant::PrivateTenant,
    unique_ids::unique_id,
    user_contexts::{SPEED_IRI, UserContexts},
};
use reqwest::StatusCode;
use serde_json::{Value, json};

/// A wrapper URL a json response linked is itself a context a later request
/// may name, on this broker or on another process on the same database; both
/// resolve it from storage, and neither records it as Cached.
#[tokio::test]
async fn a_minted_wrapper_url_named_back_is_resolved_from_storage_on_every_process() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:minted-wrapper");
    create_device(tenant.client(), &device, &contexts.terms).await;
    let answered = get(tenant.client(), &format!("entities/{device}"), "application/json", Some(&contexts.terms)).await;
    assert_eq!(answered.status(), StatusCode::OK);
    let wrapper = single_context_link(answered.headers());
    assert!(
        wrapper.contains(CONTEXTS_PATH),
        "a user context without the core is linked through a wrapper: {wrapper}"
    );

    for entities in [broker_url("entities"), second_broker_url("entities")] {
        let id = unique_id("urn:ngsi-ld:Device:named-back");
        let created = tenant
            .client()
            .post(&entities)
            .header("Content-Type", "application/json")
            .header("Link", context_link(&wrapper))
            .json(&json!({ "id": id, "type": "Device", "speed": { "type": "Property", "value": 5 } }))
            .send()
            .await
            .expect("create request failed");
        assert_eq!(created.status(), StatusCode::CREATED, "{entities}: {:?}", created.text().await);
        let stored: Value = stored_entity(tenant.client(), &id).await;
        assert_eq!(stored[SPEED_IRI]["value"], json!(5), "{entities}: the wrapper expanded the term: {stored}");
    }
    assert!(
        cached_urls(tenant.client()).await.iter().all(|url| !url.contains(CONTEXTS_PATH)),
        "no process recorded a Cached copy of the broker's own wrapper"
    );
    contexts.shutdown().await;
    tenant.finish().await;
}
