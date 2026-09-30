//! What a remote `@context` response must be for the broker to read it.
//!
//! Andromeda reads `application/json`, `application/ld+json`, any `+json`
//! type, `text/plain` (every raw GitHub-hosted data-model context) and a
//! response without `Content-Type` (the ETSI suite's own context server) as
//! JSON. Any other media type, without a JSON-LD alternate, is a context that
//! could not be obtained: `LdContextNotAvailable` (clause 5.5.6). A JSON body
//! without a top-level `@context` is an invalid one: `BadRequestData`.

use crate::{
    context_api::cached_urls,
    context_origin::{ContextOrigin, OriginResponse},
    polling::eventually,
    private_tenant::PrivateTenant,
    problem_types::{BAD_REQUEST_DATA_TYPE, LD_CONTEXT_NOT_AVAILABLE_TYPE},
    remote::{REMOTE_SPEED_IRI, create_on_broker, speed_context},
};
use reqwest::StatusCode;
use serde_json::json;

/// Study F1 and F2, and every JSON-shaped media type: the context is read, its
/// term expands, and the download is recorded as Cached.
#[tokio::test]
async fn a_context_served_as_any_json_shaped_type_or_without_a_content_type_is_read() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;
    let cases = [
        ("/text-plain.jsonld", Some("text/plain")),
        ("/text-plain-charset.jsonld", Some("text/plain; charset=utf-8")),
        ("/bare.jsonld", None),
        ("/json.jsonld", Some("application/json")),
        ("/ld-json.jsonld", Some("application/ld+json")),
        ("/suffix.jsonld", Some("application/vnd.example+json")),
    ];

    for (path, content_type) in cases {
        let url = origin.serve(path, OriginResponse::document(content_type, speed_context())).await;
        let (status, stored) = create_on_broker(tenant.client(), &url).await;
        assert_eq!(status, StatusCode::CREATED, "{path}: {stored}");
        assert_eq!(
            stored[REMOTE_SPEED_IRI]["value"],
            json!(1),
            "{path}: the term expanded through the context: {stored}"
        );
        let client = tenant.client();
        let recorded = &url;
        eventually("the download is recorded as Cached", move || async move {
            cached_urls(client).await.contains(recorded).then_some(())
        })
        .await;
    }
    origin.shutdown().await;
    tenant.finish().await;
}

/// Study F3: a media type no context is read from, with no alternate, is a
/// context that could not be obtained, and nothing is recorded.
#[tokio::test]
async fn a_context_served_as_a_non_json_type_without_an_alternate_is_not_available() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;

    for (path, content_type) in [("/binary.jsonld", "application/octet-stream"), ("/page.html", "text/html")] {
        let url = origin.serve(path, OriginResponse::document(Some(content_type), speed_context())).await;
        let (status, problem) = create_on_broker(tenant.client(), &url).await;
        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT, "{content_type}: {problem}");
        assert_eq!(problem["type"], LD_CONTEXT_NOT_AVAILABLE_TYPE.as_str(), "{content_type}");
        assert!(!cached_urls(tenant.client()).await.contains(&url), "{content_type} is not recorded");
    }
    origin.shutdown().await;
    tenant.finish().await;
}

/// Study F8: a JSON body without a top-level `@context` is an invalid context,
/// and a body that is not JSON under a JSON media type is one too.
#[tokio::test]
async fn a_json_body_without_a_context_or_a_body_that_is_not_json_is_bad_request_data() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;
    let cases = [
        ("/no-context.jsonld", json!({ "speed": REMOTE_SPEED_IRI }).to_string()),
        ("/not-json.jsonld", "this is not JSON".to_owned()),
    ];

    for (path, body) in cases {
        let url = origin.serve(path, OriginResponse::document(Some("application/json"), body)).await;
        let (status, problem) = create_on_broker(tenant.client(), &url).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {problem}");
        assert_eq!(problem["type"], BAD_REQUEST_DATA_TYPE.as_str(), "{path}");
    }
    origin.shutdown().await;
    tenant.finish().await;
}
