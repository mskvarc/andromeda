//! A zero limit without count and a count through options are refused.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Query Context Source Registrations naming a type with limit 0 and no count answers 400 `BadRequestData`, and
/// with options=count answers 400 `InvalidRequest`
/// Clause: 6.3.13, 6.3.11, 6.3.20, Table 6.3.11-1
#[gtest]
#[tokio::test]
async fn n_csr_qry_9015_a_zero_limit_without_count_and_a_count_through_options_are_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?.with_accept(Accept::new(NgsiLdMediaType::Json));
    // The client refuses a zero limit without count before sending, so it is sent as written.
    let zero_limit = request_to(&broker, Method::GET, &format!("csourceRegistrations?type=Vehicle_{namespace}&limit=0"))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let zero_limit_answer = broker.send_unchecked(zero_limit).await;
    expect_that!(&zero_limit_answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write count as an options keyword, so it is sent as written.
    let options_count = request_to(&broker, Method::GET, &format!("csourceRegistrations?type=Vehicle_{namespace}&options=count"))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let options_count_answer = broker.send_unchecked(options_count).await;
    expect_that!(&options_count_answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
