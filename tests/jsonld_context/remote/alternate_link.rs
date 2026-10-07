//! Following the JSON-LD alternate a non-JSON response names.
//!
//! JSON-LD 1.1 section 6.2 lets "a non-JSON resource (i.e., one using a media
//! type other than application/json or a derivative) [...] return the alternate
//! location using a Link Header with: rel="alternate", and
//! type="application/ld+json"". Annex C.8 names `https://schema.org` as a
//! context, which answers exactly that way. The broker follows one such hop,
//! records the download under the URL the request named, and follows no second
//! one.

use crate::{
    context_api::cached_urls,
    context_origin::{ContextOrigin, OriginResponse},
    polling::eventually,
    private_tenant::PrivateTenant,
    problem_types::LD_CONTEXT_NOT_AVAILABLE_TYPE,
    remote::{REMOTE_SPEED_IRI, create_on_broker, speed_context},
};
use axum::http::header::LINK;
use reqwest::StatusCode;
use serde_json::json;

/// The parameters a JSON-LD alternate link carries.
const ALTERNATE: &str = r#"; rel="alternate"; type="application/ld+json""#;

/// An HTML page naming one JSON-LD alternate is replaced by it; the
/// term expands through the alternate, and the entry is recorded under the URL
/// the request named.
#[tokio::test]
async fn an_html_page_naming_a_json_ld_alternate_is_read_through_it() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;
    origin
        .serve("/alternate.jsonld", OriginResponse::document(Some("application/ld+json"), speed_context()))
        .await;
    let page = origin
        .serve(
            "/page",
            OriginResponse::document(Some("text/html"), "<html></html>").with_header(LINK, format!("</alternate.jsonld>{ALTERNATE}")),
        )
        .await;

    let (status, stored) = create_on_broker(tenant.client(), &page).await;

    assert_eq!(status, StatusCode::CREATED, "{stored}");
    assert_eq!(stored[REMOTE_SPEED_IRI]["value"], json!(1), "speed expanded through the alternate: {stored}");
    let client = tenant.client();
    let requested = &page;
    eventually("the download is recorded under the requested URL", move || async move {
        cached_urls(client).await.contains(requested).then_some(())
    })
    .await;
    assert_eq!(origin.fetch_count("/page"), 1, "the page is fetched once");
    assert_eq!(origin.fetch_count("/alternate.jsonld"), 1, "the alternate is fetched once");
    origin.shutdown().await;
    tenant.finish().await;
}

/// An absolute alternate target is followed as well as a relative one.
#[tokio::test]
async fn an_absolute_alternate_target_is_followed() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;
    let alternate = origin
        .serve("/elsewhere/context.jsonld", OriginResponse::document(Some("application/json"), speed_context()))
        .await;
    let page = origin
        .serve(
            "/landing",
            OriginResponse::document(Some("text/html"), "<html></html>").with_header(LINK, format!("<{alternate}>{ALTERNATE}")),
        )
        .await;

    let (status, stored) = create_on_broker(tenant.client(), &page).await;

    assert_eq!(status, StatusCode::CREATED, "{stored}");
    assert_eq!(stored[REMOTE_SPEED_IRI]["value"], json!(1), "{stored}");
    origin.shutdown().await;
    tenant.finish().await;
}

/// An alternate that is itself HTML is not followed again,
/// two alternates state no context, and an alternate of another media type
/// is none; each is a context that could not be obtained.
#[tokio::test]
async fn a_second_hop_two_alternates_or_a_non_json_ld_alternate_are_not_available() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;
    origin
        .serve("/third.jsonld", OriginResponse::document(Some("application/ld+json"), speed_context()))
        .await;
    origin
        .serve(
            "/second",
            OriginResponse::document(Some("text/html"), "<html></html>").with_header(LINK, format!("</third.jsonld>{ALTERNATE}")),
        )
        .await;
    let chained = origin
        .serve(
            "/first",
            OriginResponse::document(Some("text/html"), "<html></html>").with_header(LINK, format!("</second>{ALTERNATE}")),
        )
        .await;
    let doubled = origin
        .serve(
            "/two",
            OriginResponse::document(Some("text/html"), "<html></html>")
                .with_header(LINK, format!("</a.jsonld>{ALTERNATE}"))
                .with_header(LINK, format!("</b.jsonld>{ALTERNATE}")),
        )
        .await;
    let other_type = origin
        .serve(
            "/other-type",
            OriginResponse::document(Some("text/html"), "<html></html>").with_header(LINK, r#"</third.jsonld>; rel="alternate"; type="text/turtle""#),
        )
        .await;

    for url in [chained, doubled, other_type] {
        let (status, problem) = create_on_broker(tenant.client(), &url).await;
        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT, "{url}: {problem}");
        assert_eq!(problem["type"], LD_CONTEXT_NOT_AVAILABLE_TYPE.as_str(), "{url}");
    }
    assert_eq!(origin.fetch_count("/third.jsonld"), 0, "the second hop is never taken");
    origin.shutdown().await;
    tenant.finish().await;
}
