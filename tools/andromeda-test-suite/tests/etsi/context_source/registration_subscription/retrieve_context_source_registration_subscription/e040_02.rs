//! ETSI `040_02`: a retrieve whose target is not a valid URI is refused with 400 `BadRequestData` (clause 5.11.4).

use crate::suite_keywords::problem_content_type;
use andromeda_test_suite::{broker, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::model::ErrorType;

/// Pins: Check that one cannot retrieve a context source registration subscription with an invalid URI, an error of
/// type `BadRequestData` shall be raised
/// Clause: 5.11.4
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112 clause
/// 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code the
/// phrase names
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e040_02_01_retrieve_context_source_registration_subscription_with_an_invalid_id() -> Result<()> {
    let broker = broker::front()?;
    let request = request_to(&broker, Method::GET, "csourceSubscriptions/invalidUri")?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(problem_content_type(&answer), some(eq("application/json")));
    verify_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_distinct_detail())
}
