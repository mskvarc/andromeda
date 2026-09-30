//! A redirected `@context` and the URL its relative references resolve against.
//!
//! JSON-LD resolves a relative reference inside a remote context against the
//! URL the document was finally read from (JSON-LD 1.1 API section 4.1.2 step
//! 5.2.1 and section 9.4.3, `documentUrl`), which after a redirect is not the
//! URL the request named. The broker records the download under the URL the
//! request named and keeps the final URL with it, so a process that never
//! downloaded the document resolves it the same way from the durable cache.

use crate::{
    broker_endpoints::{broker_url, second_broker_url},
    context_api::cached_urls,
    context_origin::{ContextOrigin, OriginResponse},
    polling::eventually,
    private_tenant::PrivateTenant,
    remote::{REMOTE_SPEED_IRI, create_under, speed_context},
};
use reqwest::StatusCode;
use serde_json::json;

/// An origin whose `/moved.jsonld` redirects to `/v2/wrapper.jsonld`, which
/// names the relative context `terms.jsonld`. Resolved against the final URL
/// that is `/v2/terms.jsonld`; resolved against the URL the request named it
/// would be `/terms.jsonld`, which the origin never serves.
async fn redirecting_origin() -> (ContextOrigin, String) {
    let origin = ContextOrigin::start().await;
    origin
        .serve("/v2/terms.jsonld", OriginResponse::document(Some("application/ld+json"), speed_context()))
        .await;
    let wrapper = origin.serve_document("/v2/wrapper.jsonld", &json!({ "@context": "terms.jsonld" })).await;
    let moved = origin.serve("/moved.jsonld", OriginResponse::redirect(wrapper)).await;
    (origin, moved)
}

/// Study F7: the relative reference in a redirected document resolves against
/// the final URL, and the download is recorded under the requested URL.
#[tokio::test]
async fn a_relative_context_in_a_redirected_document_resolves_against_the_final_url() {
    let tenant = PrivateTenant::provisioned().await;
    let (origin, moved) = redirecting_origin().await;

    let (status, stored) = create_under(tenant.client(), &broker_url("entities"), &moved).await;

    assert_eq!(status, StatusCode::CREATED, "{stored}");
    assert_eq!(stored[REMOTE_SPEED_IRI]["value"], json!(1), "speed expanded through /v2/terms.jsonld: {stored}");
    assert_eq!(origin.fetch_count("/v2/terms.jsonld"), 1, "the reference resolved under /v2/");
    let client = tenant.client();
    let requested = &moved;
    eventually("the download is recorded under the requested URL", move || async move {
        cached_urls(client).await.contains(requested).then_some(())
    })
    .await;
    origin.shutdown().await;
    tenant.finish().await;
}

/// The second broker, which never downloaded the document, resolves its
/// relative reference from the durable cache alone -- the origin is gone by
/// then -- against the URL the redirect led to.
#[tokio::test]
async fn a_redirected_context_read_back_from_the_durable_cache_resolves_against_the_final_url() {
    let tenant = PrivateTenant::provisioned().await;
    let (origin, moved) = redirecting_origin().await;
    let (status, stored) = create_under(tenant.client(), &broker_url("entities"), &moved).await;
    assert_eq!(status, StatusCode::CREATED, "the first broker downloads through the redirect: {stored}");
    let client = tenant.client();
    let downloads = [moved.clone(), origin.url("/v2/terms.jsonld")]; // clone: the requested URL is used again below
    let recorded = &downloads;
    eventually("both downloads are recorded durably", move || async move {
        let cached = cached_urls(client).await;
        recorded.iter().all(|url| cached.contains(url)).then_some(())
    })
    .await;
    let terms_fetches = origin.fetch_count("/v2/terms.jsonld");
    origin.shutdown().await;

    let (status, stored) = create_under(tenant.client(), &second_broker_url("entities"), &moved).await;

    assert_eq!(status, StatusCode::CREATED, "the second broker resolves from the durable cache: {stored}");
    assert_eq!(stored[REMOTE_SPEED_IRI]["value"], json!(1), "{stored}");
    assert_eq!(terms_fetches, 1, "the origin served the terms once, to the first broker");
    tenant.finish().await;
}
