//! A method a resource does not serve gets a 405 problem document.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    broker,
    config::BrokerRole,
    problem::{answered, instance_of, problem},
    raw_http::send_bytes,
    wire::{header_values, request_to},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ALLOW, CONTENT_TYPE},
};
use ngsi_ld::model::ErrorType;

/// Pins: a method a resource does not serve is answered 405 with a `MethodNotAllowed` problem document naming the
/// refused method and the request target, with an Allow header naming what the resource serves; a HEAD on a
/// POST-only resource gets the document with no body; the length and media refusals keep their bare body-less
/// answers
/// Clause: 5.5.3, 6.3.2, 6.3.4, 6.3.5
/// Recorded: Table 6.3.2-1 lists 405 among the HTTP binding errors but names no error type for it, so
/// `MethodNotAllowed` is Andromeda's own, the same move `RequestEntityTooLarge` is for 413; the Allow header is IETF
/// RFC 9110 clause 15.5.6
#[gtest]
#[tokio::test]
async fn o_cmn_err_9003_a_method_a_resource_does_not_serve_gets_a_405_problem_document() -> Result<()> {
    let broker = broker::front()?;
    let method_not_allowed = "https://uri.etsi.org/ngsi-ld/errors/MethodNotAllowed".parse::<ErrorType>()?;
    // No typed operation posts to the tenant resource, which serves other methods.
    let answer = broker.send_unchecked(request_to(&broker, Method::POST, "tenants/acme")?).await;
    expect_that!(
        &answer,
        problem(StatusCode::METHOD_NOT_ALLOWED, method_not_allowed)
            .with_title()
            .with_detail_containing("POST")
            .with_header_containing(ALLOW, "GET")
            .with_header_containing(ALLOW, "PATCH")
    );
    let allow = answer.answered_headers().map(|headers| header_values(headers, &ALLOW)).unwrap_or_default();
    expect_that!(allow, each(not(contains_substring("POST"))));
    let details = answer.answered_problem();
    expect_that!(details.and_then(|details| details.title.as_deref()), some(eq("Method not allowed")));
    expect_that!(details.and_then(|details| details.status), some(eq(405)));
    expect_that!(instance_of(&answer), some(eq("/ngsi-ld/v1/tenants/acme")));
    // No typed operation sends a HEAD.
    let answer_2 = broker.send_unchecked(request_to(&broker, Method::HEAD, "entityOperations/create")?).await;
    expect_that!(
        &answer_2,
        answered(StatusCode::METHOD_NOT_ALLOWED).with_header_containing(ALLOW, "POST").without_body()
    );
    // The HTTP client completes a POST without a body with `Content-Length: 0`, so the message is written to the socket.
    let answer_3 = send_bytes(BrokerRole::Front, &Method::POST, "entities", &[("Content-Type", "application/json")], b"").await?;
    expect_that!(answer_3.status(), eq(StatusCode::LENGTH_REQUIRED));
    expect_that!(answer_3.body().len(), eq(0));
    // The body goes as text/plain, which no typed operation sends.
    let request_4 = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("text/plain"))
        .body(Bytes::from_static(b"not json"));
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, answered(StatusCode::UNSUPPORTED_MEDIA_TYPE).without_body());
    Ok(())
}
