//! Every content coding through the built process stack, on request bodies the
//! stack decodes and on responses it encodes (RFC 9110 section 8.4).

use crate::stack::HttpStack;
use andromeda_configuration::{
    human_byte_size::HumanByteSize,
    schema::{content_coding::ContentCoding, http::HttpStackConfig, request_limits::RequestLimitsConfig},
};
use andromeda_http_extraction::request_payload::RequestPayload;
use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::Request,
    http::{
        HeaderValue,
        StatusCode,
        header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
    routing::post,
};
use enumset::EnumSet;
use std::{convert::Infallible, error::Error, future::ready};
use tower::{Layer, ServiceExt, service_fn};
use tower_http::{compression::CompressionLayer, decompression::DecompressionLayer, normalize_path::NormalizePath};

/// The request body limit the stack under test enforces on decoded bytes.
const BODY_LIMIT: HumanByteSize = HumanByteSize::kib(1);

/// The `Content-Encoding` token of a coding (RFC 9110 section 8.4.1).
const fn token(coding: ContentCoding) -> &'static str {
    match coding {
        ContentCoding::Gzip => "gzip",
        ContentCoding::Brotli => "br",
        ContentCoding::Deflate => "deflate",
        ContentCoding::Zstd => "zstd",
    }
}

/// A JSON document of `padding` repeated bytes, which every coding shrinks
/// well below its length.
fn document(padding: usize) -> Bytes {
    Bytes::from(format!("{{\"padding\":\"{}\"}}", "x".repeat(padding)))
}

/// A handler answering with the payload body it read.
async fn echo(RequestPayload(body): RequestPayload) -> Response {
    ([(CONTENT_TYPE, "application/json")], body).into_response()
}

/// The process and API stacks, every coding accepted both ways, over the echo route.
fn router() -> Result<NormalizePath<Router>, Box<dyn Error>> {
    let limits = RequestLimitsConfig {
        max_body: BODY_LIMIT,
        ..RequestLimitsConfig::default()
    };
    let stack = HttpStack::new(HttpStackConfig::default(), limits)?;
    Ok(stack.apply_process(stack.apply_api(Router::new().route("/echo", post(echo)))))
}

/// `body` encoded with `coding`, by the `tower-http` response encoder.
async fn encode(coding: ContentCoding, body: Bytes) -> Result<Bytes, Box<dyn Error>> {
    let encoder = CompressionLayer::new().layer(service_fn(move |_request: Request| {
        ready(Ok::<_, Infallible>(Response::new(Body::from(body.clone()))))
    }));
    let request = Request::get("/").header(ACCEPT_ENCODING, token(coding)).body(Body::empty())?;
    let response = encoder.oneshot(request).await?;
    Ok(to_bytes(Body::new(response.into_body()), usize::MAX).await?)
}

/// `body` decoded from `coding`, by the `tower-http` response decoder.
async fn decode(coding: ContentCoding, body: Bytes) -> Result<Bytes, Box<dyn Error>> {
    let decoder = DecompressionLayer::new().layer(service_fn(move |_request: Request| {
        let mut response = Response::new(Body::from(body.clone()));
        response.headers_mut().insert(CONTENT_ENCODING, HeaderValue::from_static(token(coding)));
        ready(Ok::<_, Infallible>(response))
    }));
    let response = decoder.oneshot(Request::get("/").body(Body::empty())?).await?;
    Ok(to_bytes(Body::new(response.into_body()), usize::MAX).await?)
}

/// A POST of `body` encoded with `coding`, declaring `declared_length` as its
/// `Content-Length`, or no length when `None`.
fn encoded_post(coding: ContentCoding, body: Bytes, declared_length: Option<usize>) -> Result<Request, Box<dyn Error>> {
    let mut request = Request::post("/echo")
        .header(CONTENT_TYPE, "application/json")
        .header(CONTENT_ENCODING, token(coding));
    if let Some(length) = declared_length {
        request = request.header(CONTENT_LENGTH, length);
    }
    Ok(request.body(Body::from(body))?)
}

/// ETSI GS CIM 009 v1.9.1 clause 6.3.4 checks the `Content-Length` the client
/// sent, which for an encoded body is its encoded length.
#[tokio::test]
async fn every_coding_decodes_a_request_body_that_declares_its_encoded_length() -> Result<(), Box<dyn Error>> {
    for coding in EnumSet::<ContentCoding>::all() {
        let original = document(512);
        let encoded = encode(coding, original.clone()).await?;
        let response = router()?.oneshot(encoded_post(coding, encoded.clone(), Some(encoded.len()))?).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK, "{coding}");
        pretty_assertions::assert_eq!(to_bytes(response.into_body(), usize::MAX).await?, original, "{coding}");
    }
    Ok(())
}

/// Clause 6.3.4: an absent `Content-Length` "shall result in just a 411 HTTP
/// status code (without any payload body)", whatever the body's coding.
#[tokio::test]
async fn an_encoded_request_body_without_a_content_length_is_refused_with_411() -> Result<(), Box<dyn Error>> {
    for coding in EnumSet::<ContentCoding>::all() {
        let encoded = encode(coding, document(512)).await?;
        let response = router()?.oneshot(encoded_post(coding, encoded, None)?).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::LENGTH_REQUIRED, "{coding}");
        pretty_assertions::assert_eq!(to_bytes(response.into_body(), usize::MAX).await?, Bytes::new(), "{coding}");
    }
    Ok(())
}

/// Clause 6.3.2 refuses a body of "too many bytes"; the limit counts the
/// decoded bytes, so a small encoding of a large document is still refused.
#[tokio::test]
async fn an_encoded_request_body_decoding_past_the_limit_is_refused_with_413() -> Result<(), Box<dyn Error>> {
    for coding in EnumSet::<ContentCoding>::all() {
        let encoded = encode(coding, document(8192)).await?;
        assert!(encoded.len() < 1024, "{coding}: the encoding fits the limit, {} bytes", encoded.len());
        let response = router()?.oneshot(encoded_post(coding, encoded.clone(), Some(encoded.len()))?).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE, "{coding}");
    }
    Ok(())
}

#[tokio::test]
async fn every_coding_encodes_a_response_the_client_accepts_in_it() -> Result<(), Box<dyn Error>> {
    for coding in EnumSet::<ContentCoding>::all() {
        let original = document(512);
        let request = Request::post("/echo")
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, original.len())
            .header(ACCEPT_ENCODING, token(coding))
            .body(Body::from(original.clone()))?;
        let response = router()?.oneshot(request).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK, "{coding}");
        pretty_assertions::assert_eq!(
            response.headers().get(CONTENT_ENCODING),
            Some(&HeaderValue::from_static(token(coding))),
            "{coding}"
        );
        let encoded = to_bytes(response.into_body(), usize::MAX).await?;
        assert!(encoded.len() < original.len(), "{coding}: the response is encoded");
        pretty_assertions::assert_eq!(decode(coding, encoded).await?, original, "{coding}");
    }
    Ok(())
}
