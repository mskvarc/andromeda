//! A malformed registration id is bad request data and an absent one not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
};

/// Pins: Retrieve Context Source Registration with the id not-a-uri answers 400 `BadRequestData`, and with a
/// well-formed URN naming no registration answers 404 `ResourceNotFound`
/// Clause: 5.10.1.4, 6.9.3.1
#[gtest]
#[tokio::test]
async fn n_csr_ret_9002_a_malformed_registration_id_is_bad_request_data_and_an_absent_one_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_absent_id = namespace.registration_id("absent")?;
    // The model refuses the id this request names, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        "csourceRegistrations/not-a-uri",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_absent_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
