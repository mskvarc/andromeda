//! How long a downloaded `@context` is used before it is fetched again.
//!
//! Clause 6.3.16 has the broker honour the `Expires` and `Cache-Control`
//! freshness a downloaded context carries, and clause 5.13.1 records a
//! download as Cached so a later resolution reads it from storage rather than
//! fetching it again. A context whose origin cannot be reached, and that was
//! never downloaded, fails the request that names it (clause 5.5.6).

use crate::{
    context_api::cached_urls,
    context_origin::{ContextOrigin, OriginResponse},
    polling::eventually,
    private_tenant::PrivateTenant,
    problem_types::LD_CONTEXT_NOT_AVAILABLE_TYPE,
    remote::{create_on_broker, speed_context},
};
use axum::http::header::CACHE_CONTROL;
use reqwest::StatusCode;

/// `max-age=0` forbids reusing the download, so a second request
/// fetches the context again.
#[tokio::test]
async fn a_context_served_with_max_age_zero_is_fetched_again() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;
    let url = origin
        .serve(
            "/max-age-zero.jsonld",
            OriginResponse::document(Some("application/ld+json"), speed_context()).with_header(CACHE_CONTROL, "max-age=0"),
        )
        .await;

    for _ in 0..2 {
        let (status, stored) = create_on_broker(tenant.client(), &url).await;
        assert_eq!(status, StatusCode::CREATED, "{stored}");
    }

    assert!(
        origin.fetch_count("/max-age-zero.jsonld") >= 2,
        "max-age=0 forces a fresh fetch for the second request"
    );
    origin.shutdown().await;
    tenant.finish().await;
}

/// A fresh download is recorded as Cached and read back from storage: two
/// requests naming it fetch it once.
#[tokio::test]
async fn a_fresh_download_is_fetched_once_for_two_requests() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;
    let url = origin
        .serve(
            "/fresh.jsonld",
            OriginResponse::document(Some("application/ld+json"), speed_context()).with_header(CACHE_CONTROL, "max-age=3600"),
        )
        .await;

    for _ in 0..2 {
        let (status, stored) = create_on_broker(tenant.client(), &url).await;
        assert_eq!(status, StatusCode::CREATED, "{stored}");
    }

    assert_eq!(origin.fetch_count("/fresh.jsonld"), 1, "the second request reads the recorded copy");
    let client = tenant.client();
    let recorded = &url;
    eventually("the download is recorded as Cached", move || async move {
        cached_urls(client).await.contains(recorded).then_some(())
    })
    .await;
    origin.shutdown().await;
    tenant.finish().await;
}

/// A context that was never downloaded and whose origin is gone
/// fails the request with 504 `LdContextNotAvailable` (Table 6.3.2-1).
#[tokio::test]
async fn a_context_whose_origin_is_unreachable_is_not_available() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;
    let url = origin
        .serve("/unreachable.jsonld", OriginResponse::document(Some("application/ld+json"), speed_context()))
        .await;
    origin.shutdown().await;

    let (status, problem) = create_on_broker(tenant.client(), &url).await;

    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT, "{problem}");
    assert_eq!(problem["type"], LD_CONTEXT_NOT_AVAILABLE_TYPE.as_str());
    tenant.finish().await;
}
