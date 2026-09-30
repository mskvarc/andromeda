//! Add @context (clause 5.13.2, binding 6.29.3.1; ETSI 050).

use crate::{
    context_api::{cached_urls, create_context, extract_id, get_context, list_contexts},
    context_origin::ContextOrigin,
    ngsi_requests::{create_entity, create_subscription, delete_entity, delete_subscription},
    polling::eventually,
    private_tenant::PrivateTenant,
    problem_types::INVALID_REQUEST_TYPE,
    unique_ids::unique_id,
};
use reqwest::StatusCode;
use serde_json::json;

/// `050_01_01`: a key-value dictionary is added as a Hosted @context.
#[tokio::test]
async fn add_valid_context_dictionary_returns_201() {
    let tenant = PrivateTenant::provisioned().await;
    let body = json!({
        "@context": {
            "A1": "urn:ngsi-ld:attributes:A1",
            "A2": "urn:ngsi-ld:attributes:A2",
            "A3": "urn:ngsi-ld:attributes:A3"
        }
    });

    let (status, location, _) = create_context(tenant.client(), &body).await;

    assert_eq!(status, StatusCode::CREATED);
    let context_id = extract_id(location.as_deref().expect("a Location header"));
    let (status, detail) = get_context(tenant.client(), &context_id, "details=true").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["kind"], "Hosted");
    tenant.finish().await;
}

/// `050_01_02`: a list of URLs is added as a Hosted @context; the documents it
/// names are served by the test's own origin.
#[tokio::test]
async fn add_valid_context_uri_list_returns_201() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;
    let first = origin
        .serve_document("/first.jsonld", &json!({ "@context": { "A1": "urn:ngsi-ld:attributes:A1" } }))
        .await;
    let second = origin
        .serve_document("/second.jsonld", &json!({ "@context": { "A2": "urn:ngsi-ld:attributes:A2" } }))
        .await;

    let (status, location, _) = create_context(tenant.client(), &json!({ "@context": [first, second] })).await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(location.is_some(), "a Location header");
    origin.shutdown().await;
    tenant.finish().await;
}

/// `050_02_01`: a body without a top-level `@context` is `InvalidRequest`.
#[tokio::test]
async fn add_invalid_context_missing_key_returns_400() {
    let tenant = PrivateTenant::provisioned().await;
    let body = json!({
        "A1": "urn:ngsi-ld:attributes:A1",
        "A2": "urn:ngsi-ld:attributes:A2",
        "A3": "urn:ngsi-ld:attributes:A3"
    });

    let (status, _, problem) = create_context(tenant.client(), &body).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(problem["type"], INVALID_REQUEST_TYPE.as_str());
    tenant.finish().await;
}

/// `050_02_02`: an empty object is refused.
#[tokio::test]
async fn add_invalid_context_empty_object_returns_400() {
    let tenant = PrivateTenant::provisioned().await;

    let (status, _, _) = create_context(tenant.client(), &json!({})).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    tenant.finish().await;
}

/// `050_03_01`: a subscription whose `@context` is an array leaves an
/// `ImplicitlyCreated` @context behind (clause 5.13.1).
#[tokio::test]
async fn add_implicitly_created_context_via_subscription() {
    let tenant = PrivateTenant::provisioned().await;
    let subscription = json!({
        "type": "Subscription",
        "entities": [{ "type": "T1" }],
        "notification": { "endpoint": { "uri": "http://127.0.0.1:1/notify" } },
        "@context": [{ "T1": "https://example.org/add/T1" }, { "speed": "https://example.org/add/speed" }]
    });

    let (status, location) = create_subscription(tenant.client(), &subscription).await;

    assert_eq!(status, StatusCode::CREATED);
    let (status, contexts) = list_contexts(tenant.client(), "kind=ImplicitlyCreated").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!contexts.is_empty(), "at least one ImplicitlyCreated @context");
    delete_subscription(tenant.client(), &extract_id(location.as_deref().expect("a Location header"))).await;
    tenant.finish().await;
}

/// `050_04_01`: a Hosted @context naming a remote URL, used by a write, has the
/// broker download that URL and record it as Cached; the Hosted entry itself
/// is resolved from storage.
#[tokio::test]
async fn add_hosted_context_with_cached_uri_refs() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;
    let context_url = origin
        .serve_document("/context.jsonld", &json!({ "@context": { "X1": "urn:ngsi-ld:attributes:X1" } }))
        .await;
    let (status, location, _) = create_context(tenant.client(), &json!({ "@context": [context_url] })).await;
    assert_eq!(status, StatusCode::CREATED);
    let hosted_id = extract_id(location.as_deref().expect("a Location header"));
    let (status, detail) = get_context(tenant.client(), &hosted_id, "details=true").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["kind"], "Hosted");
    let hosted_url = detail["URL"].as_str().expect("a serving URL").to_owned();

    let entity_id = unique_id("urn:ngsi-ld:Testing:cached-ref");
    let entity = json!({
        "id": entity_id,
        "type": "Testing",
        "X1": { "type": "Property", "value": "test" },
        "@context": [hosted_url]
    });
    let (status, _) = create_entity(tenant.client(), &entity).await;

    assert_eq!(status, StatusCode::CREATED);
    let client = tenant.client();
    let referenced = &context_url;
    eventually("the referenced URL is recorded as Cached", move || async move {
        cached_urls(client).await.contains(referenced).then_some(())
    })
    .await;
    assert!(
        !cached_urls(tenant.client()).await.contains(&hosted_url),
        "the broker's own document is never recorded as Cached"
    );
    delete_entity(tenant.client(), &entity_id).await;
    origin.shutdown().await;
    tenant.finish().await;
}

/// A single URL string is added as a Hosted @context.
#[tokio::test]
async fn add_context_single_string_uri_returns_201() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;
    let context_url = origin
        .serve_document("/single.jsonld", &json!({ "@context": { "A1": "urn:ngsi-ld:attributes:A1" } }))
        .await;

    let (status, location, _) = create_context(tenant.client(), &json!({ "@context": context_url })).await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(location.is_some(), "a Location header");
    origin.shutdown().await;
    tenant.finish().await;
}
