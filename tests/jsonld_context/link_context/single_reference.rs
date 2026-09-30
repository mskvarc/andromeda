//! A context `Link` naming one URL itself (study B1, B2 and B4).

use crate::{
    broker_endpoints::broker_url,
    context_api::implicit_context_ids,
    context_links::single_context_link,
    core_contexts::{CORE_V18, core},
    ngsi_requests::{create_device, get},
    private_tenant::PrivateTenant,
    unique_ids::unique_id,
    user_contexts::{DEVICE_IRI, UserContexts},
};
use reqwest::StatusCode;
use serde_json::json;

/// Study B1: with no request context, and with only an older core release,
/// the one link names the v1.9 core `@context`, typed `application/ld+json`.
#[tokio::test]
async fn with_no_user_context_the_one_link_names_the_core_context() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:single-core");
    create_device(tenant.client(), &device, &contexts.terms).await;

    for link in [None, Some(CORE_V18)] {
        let response = get(tenant.client(), &format!("entities/{device}"), "application/json", link).await;
        assert_eq!(response.status(), StatusCode::OK, "{link:?}");
        let link_header = response
            .headers()
            .get("link")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        assert_eq!(single_context_link(response.headers()), core(), "{link:?}");
        assert!(link_header.contains(r#"type="application/ld+json""#), "{link_header}");
    }
    assert!(implicit_context_ids(tenant.client()).await.is_empty(), "no wrapper is hosted");
    contexts.shutdown().await;
    tenant.finish().await;
}

/// Study B2 and ETSI `040_01`: a context that ends with the core `@context` is
/// linked as itself, on a single retrieval and on a query, and no wrapper is
/// hosted for it.
#[tokio::test]
async fn a_context_ending_with_the_core_context_is_linked_as_itself() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:single-compound");
    create_device(tenant.client(), &device, &contexts.terms).await;

    for (path, shape) in [
        (format!("entities/{device}"), &contexts.wrapper),
        (format!("entities?type=Device&id={device}"), &contexts.wrapper),
        (format!("entities/{device}"), &contexts.outer),
        (format!("entities/{device}"), &contexts.relative),
    ] {
        let response = get(tenant.client(), &path, "application/json", Some(shape)).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(&single_context_link(response.headers()), shape, "{path}");
    }
    assert!(implicit_context_ids(tenant.client()).await.is_empty(), "no wrapper is hosted");
    contexts.shutdown().await;
    tenant.finish().await;
}

/// Study B4: an `application/ld+json` POST query whose body context is one URL
/// ending with the core `@context`, answered as `application/json`, links that
/// URL; one whose body context is the core `@context` links the core.
#[tokio::test]
async fn a_post_query_with_one_body_context_answered_as_json_links_it() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:single-post");
    create_device(tenant.client(), &device, &contexts.terms).await;

    for (body_context, expected) in [
        (json!(contexts.wrapper), contexts.wrapper.as_str()),
        (json!([contexts.wrapper]), contexts.wrapper.as_str()),
        (json!(core()), core()),
    ] {
        let response = tenant
            .client()
            .post(broker_url("entityOperations/query"))
            .header("Content-Type", "application/ld+json")
            .header("Accept", "application/json")
            .json(&json!({ "type": "Query", "entities": [{ "type": DEVICE_IRI, "id": device }], "@context": body_context }))
            .send()
            .await
            .expect("POST query failed");
        assert_eq!(response.status(), StatusCode::OK, "{body_context}");
        assert_eq!(single_context_link(response.headers()), expected, "{body_context}");
    }
    assert!(implicit_context_ids(tenant.client()).await.is_empty(), "no wrapper is hosted");
    contexts.shutdown().await;
    tenant.finish().await;
}
