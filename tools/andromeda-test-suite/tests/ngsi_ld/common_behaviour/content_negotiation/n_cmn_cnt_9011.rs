//! A declared zero length is an invalid request.

use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{CONTENT_LENGTH, CONTENT_TYPE},
};
use ngsi_ld::model::ErrorType;

/// Pins: Create Entity, batch create, batch delete and Add @context declaring Content-Length 0 answer 400
/// `InvalidRequest`, an empty body being no JSON document
/// Clause: 6.3.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_9011_a_declared_zero_length_is_an_invalid_request() -> Result<()> {
    let broker = broker::front()?;
    // The request declares an empty body, which no typed operation sends, so it is sent as written.
    let request = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_LENGTH, HeaderValue::from_static("0"))
        .body(Bytes::from(""));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    // The request declares an empty body, which no typed operation sends, so it is sent as written.
    let request_2 = request_to(&broker, Method::POST, "entityOperations/create")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_LENGTH, HeaderValue::from_static("0"))
        .body(Bytes::from(""));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    // The request declares an empty body, which no typed operation sends, so it is sent as written.
    let request_3 = request_to(&broker, Method::POST, "entityOperations/delete")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_LENGTH, HeaderValue::from_static("0"))
        .body(Bytes::from(""));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    // The request declares an empty body, which no typed operation sends, so it is sent as written.
    let request_4 = request_to(&broker, Method::POST, "jsonldContexts")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_LENGTH, HeaderValue::from_static("0"))
        .body(Bytes::from(""));
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
