//! Serve @context (clause 5.13.4, binding 6.30.3.1; ETSI 053).

use crate::{
    context_api::{create_hosted_context, create_implicit_context, delete_context, find_cached_context_id, get_context, number_of_hits, setup_cached_context},
    ngsi_requests::{create_entity, delete_entity, delete_subscription},
    private_tenant::PrivateTenant,
    problem_types::RESOURCE_NOT_FOUND_TYPE,
    unique_ids::unique_id,
};
use reqwest::StatusCode;
use serde_json::json;

/// `053_01_01` and `053_01_02`: without `details`, and with `details=false`,
/// Serve answers the stored document.
#[tokio::test]
async fn serve_context_without_details_returns_the_document() {
    let tenant = PrivateTenant::provisioned().await;
    let context_id = create_hosted_context(tenant.client()).await;

    for query in ["", "details=false"] {
        let (status, body) = get_context(tenant.client(), &context_id, query).await;
        assert_eq!(status, StatusCode::OK, "{query}");
        assert_eq!(
            body,
            json!({ "@context": { "A1": "urn:ngsi-ld:attributes:A1", "A2": "urn:ngsi-ld:attributes:A2", "A3": "urn:ngsi-ld:attributes:A3" } }),
            "{query}"
        );
    }
    tenant.finish().await;
}

/// `053_02_01`: `details=true` answers the clause 5.13.3.5 metadata.
#[tokio::test]
async fn serve_context_details_true_returns_metadata() {
    let tenant = PrivateTenant::provisioned().await;
    let context_id = create_hosted_context(tenant.client()).await;

    let (status, body) = get_context(tenant.client(), &context_id, "details=true").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["kind"], "Hosted");
    assert_eq!(body["localId"], json!(context_id));
    assert!(body.get("URL").is_some(), "a serving URL: {body}");
    assert!(body.get("createdAt").is_some(), "a createdAt: {body}");
    tenant.finish().await;
}

/// `053_03_01`: an unknown identifier is `ResourceNotFound`.
#[tokio::test]
async fn serve_unknown_context_returns_404() {
    let tenant = PrivateTenant::provisioned().await;

    let (status, body) = get_context(tenant.client(), "urn:ngsi-ld:unknown:ctx:12345", "").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
    tenant.finish().await;
}

/// `053_04_01`: using a Hosted @context counts toward its `numberOfHits`, and
/// the count is current the instant the using request returns.
#[tokio::test]
async fn serve_hosted_number_of_hits_increases() {
    let tenant = PrivateTenant::provisioned().await;
    let context_id = create_hosted_context(tenant.client()).await;
    let (status, detail) = get_context(tenant.client(), &context_id, "details=true").await;
    assert_eq!(status, StatusCode::OK);
    let initial_hits = detail["numberOfHits"].as_u64().unwrap_or(0);
    let context_url = detail["URL"].as_str().expect("a serving URL").to_owned();
    let entity_id = unique_id("urn:ngsi-ld:Testing:hits");

    let (status, _) = create_entity(
        tenant.client(),
        &json!({ "id": entity_id, "type": "Testing", "A1": { "type": "Property", "value": "value of A1" }, "@context": [context_url] }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let hits = number_of_hits(tenant.client(), &context_id).await;
    assert!(hits > initial_hits, "numberOfHits increases at once: {hits} > {initial_hits}");
    delete_entity(tenant.client(), &entity_id).await;
    tenant.finish().await;
}

/// `053_07_01`: an `ImplicitlyCreated` @context answers its metadata.
#[tokio::test]
async fn serve_implicitly_created_details_true() {
    let tenant = PrivateTenant::provisioned().await;
    let (subscription_id, context_id) = create_implicit_context(tenant.client()).await;

    let (status, body) = get_context(tenant.client(), &context_id, "details=true").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["kind"], "ImplicitlyCreated");
    assert!(body.get("URL").is_some(), "a serving URL: {body}");
    assert!(body.get("createdAt").is_some(), "a createdAt: {body}");
    delete_subscription(tenant.client(), &subscription_id).await;
    tenant.finish().await;
}

/// `053_08_01`: using an `ImplicitlyCreated` @context counts toward its
/// `numberOfHits`.
#[tokio::test]
async fn serve_implicitly_created_number_of_hits_increases() {
    let tenant = PrivateTenant::provisioned().await;
    let (subscription_id, context_id) = create_implicit_context(tenant.client()).await;
    let (_, detail) = get_context(tenant.client(), &context_id, "details=true").await;
    let initial_hits = detail["numberOfHits"].as_u64().unwrap_or(0);
    let context_url = detail["URL"].as_str().expect("a serving URL").to_owned();
    let entity_id = unique_id("urn:ngsi-ld:T1:implicit-hits");

    let (status, _) = create_entity(
        tenant.client(),
        &json!({ "id": entity_id, "type": "T1", "speed": { "type": "Property", "value": 1 }, "@context": [context_url] }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let hits = number_of_hits(tenant.client(), &context_id).await;
    assert!(hits > initial_hits, "numberOfHits increases at once: {hits} > {initial_hits}");
    delete_entity(tenant.client(), &entity_id).await;
    delete_subscription(tenant.client(), &subscription_id).await;
    tenant.finish().await;
}

/// `053_05_01`: a Cached @context keeps answering its metadata after its
/// origin is gone, although it cannot be reloaded.
#[tokio::test]
async fn serve_cached_context_after_server_down() {
    let tenant = PrivateTenant::provisioned().await;
    let (origin, context_url) = setup_cached_context(tenant.client()).await;
    let cached_id = find_cached_context_id(tenant.client(), &context_url).await;
    origin.shutdown().await;

    let (reload, _) = delete_context(tenant.client(), &cached_id, "reload=true").await;
    let (status, body) = get_context(tenant.client(), &cached_id, "details=true").await;

    assert_eq!(reload, StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["kind"], "Cached");
    assert_eq!(body["URL"], json!(context_url));
    assert!(body.get("createdAt").is_some(), "a createdAt: {body}");
    tenant.finish().await;
}

/// `053_06_01`: a second use of a Cached @context counts toward its
/// `numberOfHits`.
#[tokio::test]
async fn serve_cached_number_of_hits_increases() {
    let tenant = PrivateTenant::provisioned().await;
    let (origin, context_url) = setup_cached_context(tenant.client()).await;
    let cached_id = find_cached_context_id(tenant.client(), &context_url).await;
    let hits_before = number_of_hits(tenant.client(), &cached_id).await;
    let entity_id = unique_id("urn:ngsi-ld:Testing:cache-hits");

    let (status, _) = create_entity(
        tenant.client(),
        &json!({ "id": entity_id, "type": "Testing", "X1": { "type": "Property", "value": "again" }, "@context": [context_url] }),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let hits = number_of_hits(tenant.client(), &cached_id).await;
    assert!(hits > hits_before, "numberOfHits increases at once: {hits} > {hits_before}");
    delete_entity(tenant.client(), &entity_id).await;
    origin.shutdown().await;
    tenant.finish().await;
}
