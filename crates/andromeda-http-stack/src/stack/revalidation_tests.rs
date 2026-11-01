//! Revalidating a compressed response through the built process stack (RFC
//! 7232 sections 2.3.3 and 4.1).

use crate::stack::HttpStack;
use andromeda_configuration::schema::{
    content_coding::ContentCoding,
    feature_mode::FeatureMode,
    http::{
        HttpStackConfig,
        compression::{CompressionConfig, CompressionQuality},
    },
    request_limits::RequestLimitsConfig,
};
use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{
        HeaderMap,
        HeaderValue,
        StatusCode,
        header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_TYPE, ETAG, IF_NONE_MATCH, VARY},
    },
    response::{IntoResponse, Response},
    routing::get,
};
use enumset::EnumSet;
use std::error::Error;
use tower::ServiceExt;

/// The strong tag the handler computes over the unencoded document.
const TAG: &str = "\"document\"";

/// A document long enough for the compression layer to encode.
fn document() -> String {
    format!("{{\"padding\":\"{}\"}}", "x".repeat(4096))
}

/// A handler that answers 304 to any `If-None-Match` and the document
/// otherwise, tagged strongly either way.
async fn tagged(headers: HeaderMap) -> Response {
    let mut response = if headers.contains_key(IF_NONE_MATCH) {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        ([(CONTENT_TYPE, "application/json")], document()).into_response()
    };
    response.headers_mut().insert(ETAG, HeaderValue::from_static(TAG));
    response
}

/// The process stack with gzip compression over the tagged route.
fn stack() -> Result<HttpStack, Box<dyn Error>> {
    let config = HttpStackConfig {
        compression: CompressionConfig {
            mode: FeatureMode::Enabled,
            algorithms: EnumSet::only(ContentCoding::Gzip),
            quality: CompressionQuality::Fastest,
        },
        ..HttpStackConfig::default()
    };
    Ok(HttpStack::new(config, RequestLimitsConfig::default())?)
}

/// The answer to a GET of the tagged route with `headers`.
async fn answer(headers: &[(&str, &str)]) -> Result<Response, Box<dyn Error>> {
    let router = stack()?.apply_process(Router::new().route("/tagged", get(tagged)));
    let mut request = Request::get("/tagged");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    Ok(router.oneshot(request.body(Body::empty())?).await?)
}

/// Whether a response names `accept-encoding` in its `Vary`.
fn varies_on_encoding(response: &Response) -> bool {
    response
        .headers()
        .get_all(VARY)
        .iter()
        .flat_map(|value| value.as_bytes().split(|byte| *byte == b','))
        .any(|member| member.trim_ascii().eq_ignore_ascii_case(b"accept-encoding"))
}

#[tokio::test]
async fn a_gzip_response_and_its_revalidation_carry_the_same_weak_tag_and_vary() -> Result<(), Box<dyn Error>> {
    let full = answer(&[(ACCEPT_ENCODING.as_str(), "gzip")]).await?;
    pretty_assertions::assert_eq!(full.status(), StatusCode::OK);
    pretty_assertions::assert_eq!(full.headers().get(CONTENT_ENCODING), Some(&HeaderValue::from_static("gzip")));
    let weak = full.headers().get(ETAG).cloned(); // clone: the 304's tag is compared against it
    pretty_assertions::assert_eq!(weak, Some(HeaderValue::from_static("W/\"document\"")));
    assert!(varies_on_encoding(&full));

    let revalidated = answer(&[(ACCEPT_ENCODING.as_str(), "gzip"), (IF_NONE_MATCH.as_str(), "W/\"document\"")]).await?;
    pretty_assertions::assert_eq!(revalidated.status(), StatusCode::NOT_MODIFIED);
    pretty_assertions::assert_eq!(revalidated.headers().get(ETAG).cloned(), weak);
    assert!(varies_on_encoding(&revalidated), "RFC 7232 section 4.1: the 304 carries the 200's Vary");
    Ok(())
}

#[tokio::test]
async fn an_identity_response_and_its_revalidation_keep_the_strong_tag_and_carry_the_same_vary() -> Result<(), Box<dyn Error>> {
    let full = answer(&[]).await?;
    pretty_assertions::assert_eq!(full.headers().get(CONTENT_ENCODING), None);
    pretty_assertions::assert_eq!(full.headers().get(ETAG), Some(&HeaderValue::from_static(TAG)));
    assert!(varies_on_encoding(&full));

    let revalidated = answer(&[(IF_NONE_MATCH.as_str(), TAG)]).await?;
    pretty_assertions::assert_eq!(revalidated.status(), StatusCode::NOT_MODIFIED);
    pretty_assertions::assert_eq!(revalidated.headers().get(ETAG), Some(&HeaderValue::from_static(TAG)));
    assert!(varies_on_encoding(&revalidated), "RFC 7232 section 4.1: the 304 carries the 200's Vary");
    Ok(())
}
