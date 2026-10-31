//! Requests the conformance tests write as text rather than through a typed operation, and the checks on their answers:
//! refusals, not found, a status alone, or a JSON body.

use andromeda_test_suite::{
    broker::BrokerClient,
    problem::{answered, problem},
    wire::{content_type, header_value, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderName,
    HeaderValue,
    Method,
    StatusCode,
    header::{CONTENT_LENGTH, CONTENT_TYPE},
};
use mediatype::MediaType;
use ngsi_ld::{
    client::{ClientError, Received, ReqwestError},
    http::UncheckedRequest,
    model::ErrorType,
};
use serde_json::Value;

/// The `Content-Type` of the error response `answer` failed with, when exactly one field line carries it.
pub(crate) fn problem_content_type<O>(answer: &Result<O, ClientError<ReqwestError>>) -> Option<&str> {
    match answer {
        Err(ClientError::Response(response)) => header_value(response.head().headers(), &CONTENT_TYPE),
        Ok(_) | Err(_) => None,
    }
}

/// A `GET` of `target`, relative to the API root and written exactly as given, refused with 400
/// `BadRequestData` and a non-empty title.
pub(crate) async fn refused_get(broker: &BrokerClient, target: &str) -> Result<()> {
    refused(broker, request_to(broker, Method::GET, target)?, ErrorType::BadRequestData).await
}

/// Sends `request`, requires 200, and answers the body as JSON.
pub(crate) async fn json_answer(broker: &BrokerClient, request: UncheckedRequest) -> Result<Value> {
    let answer = broker.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::OK))?;
    Ok(serde_json::from_slice(answer?.body())?)
}

/// The `NGSILD-Results-Count` header of a query answer, when exactly one field line carries it.
pub(crate) fn results_count<V>(received: &Received<V>) -> Option<&str> {
    header_value(received.head.headers(), &HeaderName::from_static("ngsild-results-count"))
}

/// Sends `request`, requires 400 with the problem type `error_type` and a title.
pub(crate) async fn refused(broker: &BrokerClient, request: UncheckedRequest, error_type: ErrorType) -> Result<()> {
    let answer = broker.send_unchecked(request).await;
    verify_that!(&answer, problem(StatusCode::BAD_REQUEST, error_type).with_title())
}

/// Sends `request`, requires 404 `ResourceNotFound` and a title.
pub(crate) async fn not_found(broker: &BrokerClient, request: UncheckedRequest) -> Result<()> {
    let answer = broker.send_unchecked(request).await;
    verify_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title())
}

/// Sends `request` with `client` and requires `status`, whatever the body: the refusals whose test names the status
/// only.
pub(crate) async fn answered_with(client: &BrokerClient, request: UncheckedRequest, status: StatusCode) -> Result<()> {
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(status))
}

/// A `POST` of `text` to `path` (relative to the API root, written exactly as given) sent as
/// `media_type`: the body an invalid case sends exactly as written.
pub(crate) fn post_text(broker: &BrokerClient, path: &str, media_type: &MediaType<'_>, text: &str) -> Result<UncheckedRequest> {
    Ok(content_type(request_to(broker, Method::POST, path)?, media_type)?.body(Bytes::copy_from_slice(text.as_bytes())))
}

/// A `POST` with no body to `path` sent as `media_type`, with `Content-Length: 0` named as the case names it: the
/// broker answers 411 to an empty request that names no length.
pub(crate) fn post_empty(broker: &BrokerClient, path: &str, media_type: &MediaType<'_>) -> Result<UncheckedRequest> {
    Ok(post_text(broker, path, media_type, "")?.append_header(CONTENT_LENGTH, HeaderValue::from_static("0")))
}

/// `request` with the first member separator of its body doubled (`"id":"...",,"type"`), the one defect that makes the
/// invalid-JSON cases' bodies not JSON.
pub(crate) fn with_doubled_separator(request: UncheckedRequest) -> UncheckedRequest {
    let text = String::from_utf8_lossy(request.as_request().body()).replacen(',', ",,", 1);
    request.body(Bytes::from(text))
}
