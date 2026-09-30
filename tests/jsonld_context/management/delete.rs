//! Delete and Reload @context (clause 5.13.5, binding 6.30.3.2; ETSI 051).

use crate::{
    context_api::{
        create_hosted_context,
        create_implicit_context,
        delete_context,
        delete_context_raw,
        find_cached_context_id,
        get_context,
        setup_cached_context,
    },
    core_contexts::core,
    ngsi_requests::delete_subscription,
    private_tenant::PrivateTenant,
    problem_types::{BAD_REQUEST_DATA_TYPE, LD_CONTEXT_NOT_AVAILABLE_TYPE, RESOURCE_NOT_FOUND_TYPE},
};
use reqwest::StatusCode;

/// `051_01_01`: a Hosted @context is deleted with 204 and no body.
#[tokio::test]
async fn delete_hosted_context_returns_204() {
    let tenant = PrivateTenant::provisioned().await;
    let context_id = create_hosted_context(tenant.client()).await;

    let (status, bytes) = delete_context_raw(tenant.client(), &context_id, "").await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(bytes.is_empty(), "a 204 carries no body");
    let (status, _) = get_context(tenant.client(), &context_id, "").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the deleted @context is gone");
    tenant.finish().await;
}

/// `051_02_01`: an unknown identifier is `ResourceNotFound`.
#[tokio::test]
async fn delete_unknown_context_returns_404() {
    let tenant = PrivateTenant::provisioned().await;

    let (status, body) = delete_context(tenant.client(), "urn:ngsi-ld:unknown:context:12345", "").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
    tenant.finish().await;
}

/// `051_04_01`: clause 5.13.5.4 settles "does not correspond to any existing
/// entry" (`ResourceNotFound`) before either reload rule, so an unknown
/// identifier with `reload=true` is a 404, not a kind mismatch.
#[tokio::test]
async fn delete_unknown_with_reload_true_returns_404() {
    let tenant = PrivateTenant::provisioned().await;

    let (status, body) = delete_context(tenant.client(), "urn:ngsi-ld:unknown:ctx", "reload=true").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
    tenant.finish().await;
}

/// `051_04_02` and `051_04_03`: an unknown identifier is a 404 with
/// `reload=false` and without `reload`.
#[tokio::test]
async fn delete_unknown_with_reload_false_or_absent_returns_404() {
    let tenant = PrivateTenant::provisioned().await;

    for query in ["reload=false", ""] {
        let (status, body) = delete_context(tenant.client(), "urn:ngsi-ld:unknown:ctx", query).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{query}: {body}");
    }
    tenant.finish().await;
}

/// `051_04_04`: a `reload` value that is not a boolean is refused.
#[tokio::test]
async fn delete_unknown_with_reload_invalid_returns_400() {
    let tenant = PrivateTenant::provisioned().await;

    let (status, _) = delete_context(tenant.client(), "urn:ngsi-ld:unknown:ctx", "reload=xxx").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    tenant.finish().await;
}

/// `051_04_05`: clause 5.13.5.4 refuses reloading anything but a Cached entry
/// as `BadRequestData`.
#[tokio::test]
async fn delete_hosted_with_reload_true_returns_400() {
    let tenant = PrivateTenant::provisioned().await;
    let context_id = create_hosted_context(tenant.client()).await;

    let (status, body) = delete_context(tenant.client(), &context_id, "reload=true").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str(), "a non-Cached reload is BadRequestData");
    tenant.finish().await;
}

/// `051_06_01`: an `ImplicitlyCreated` @context is deleted with 204.
#[tokio::test]
async fn delete_implicitly_created_context_returns_204() {
    let tenant = PrivateTenant::provisioned().await;
    let (subscription_id, context_id) = create_implicit_context(tenant.client()).await;

    let (status, _) = delete_context_raw(tenant.client(), &context_id, "").await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    delete_subscription(tenant.client(), &subscription_id).await;
    tenant.finish().await;
}

/// `051_07_01`: an `ImplicitlyCreated` @context is not Cached, so reloading it
/// is `BadRequestData`.
#[tokio::test]
async fn delete_implicitly_created_with_reload_returns_400() {
    let tenant = PrivateTenant::provisioned().await;
    let (subscription_id, context_id) = create_implicit_context(tenant.client()).await;

    let (status, body) = delete_context(tenant.client(), &context_id, "reload=true").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str(), "a non-Cached reload is BadRequestData");
    delete_subscription(tenant.client(), &subscription_id).await;
    tenant.finish().await;
}

/// `051_08_01` and `051_08_02`: the embedded core @context cannot be deleted.
#[tokio::test]
async fn delete_core_context_without_reload_returns_400() {
    let tenant = PrivateTenant::provisioned().await;

    for query in ["", "reload=false"] {
        let (status, body) = delete_context(tenant.client(), core(), query).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str(), "{query}");
    }
    tenant.finish().await;
}

/// `051_09_01`: the embedded core @context is listed `ImplicitlyCreated`
/// (clause 5.13.1) and has no URL to re-download from, so reloading it is
/// `BadRequestData` and it stays served.
#[tokio::test]
async fn delete_and_reload_core_context_returns_400() {
    let tenant = PrivateTenant::provisioned().await;
    let (status, _) = get_context(tenant.client(), core(), "").await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = delete_context(tenant.client(), core(), "reload=true").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    let (status, _) = get_context(tenant.client(), core(), "").await;
    assert_eq!(status, StatusCode::OK, "the refusal leaves the embedded core @context served");
    tenant.finish().await;
}

/// `051_03_01`: a Cached @context is deleted with 204 and no body.
#[tokio::test]
async fn delete_cached_context_returns_204() {
    let tenant = PrivateTenant::provisioned().await;
    let (origin, context_url) = setup_cached_context(tenant.client()).await;
    let cached_id = find_cached_context_id(tenant.client(), &context_url).await;

    let (status, bytes) = delete_context_raw(tenant.client(), &cached_id, "").await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(bytes.is_empty(), "a 204 carries no body");
    origin.shutdown().await;
    tenant.finish().await;
}

/// `051_05_01`: reloading a Cached @context whose origin is gone is
/// `LdContextNotAvailable` (clause 5.13.5.4), and the entry stays.
#[tokio::test]
async fn delete_cached_reload_server_unavailable_returns_504() {
    let tenant = PrivateTenant::provisioned().await;
    let (origin, context_url) = setup_cached_context(tenant.client()).await;
    let cached_id = find_cached_context_id(tenant.client(), &context_url).await;
    origin.shutdown().await;

    let (status, body) = delete_context(tenant.client(), &cached_id, "reload=true").await;

    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(body["type"], LD_CONTEXT_NOT_AVAILABLE_TYPE.as_str());
    let (status, detail) = get_context(tenant.client(), &cached_id, "details=true").await;
    assert_eq!(status, StatusCode::OK, "the failed reload removed nothing");
    assert_eq!(detail["kind"], "Cached");
    tenant.finish().await;
}
