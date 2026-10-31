//! A join level of zero or below is bad request data and a non integer is invalid request.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Retrieve Entity with a `joinLevel` of 0 or below is refused with `BadRequestData`, and one that is not an
/// integer (1.5, abc) with `InvalidRequest`
/// Clause: 6.5.3.1, 4.5.23, 5.5.2
/// Recorded: Table 6.5.3.1-1 restricts `joinLevel` to a positive integer and no clause names the error for a URL
/// parameter outside it, so the files are kind O; Table 5.5.2-1 defines `BadRequestData` as input data that "does
/// not meet the requirements of the operation" (an integer below 1) and `InvalidRequest` as a request
/// "syntactically invalid" (a value that is no integer at all). Ruled by the user (2026-10-05)
#[gtest]
#[tokio::test]
async fn o_lnk_ret_0011_a_join_level_of_zero_or_below_is_bad_request_data_and_a_non_integer_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_a_id = namespace.urn("Building", "a")?;
    let building_type = namespace.entity_type("Building")?;
    let building_a = Entity::builder().id(building_a_id.clone()).entity_type(building_type).build()?;
    broker.entities().create(&building_a).send().await?.done()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities/{building_a_id}?join=inline&joinLevel=0"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities/{building_a_id}?join=inline&joinLevel=-1"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities/{building_a_id}?join=inline&joinLevel=1.5"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities/{building_a_id}?join=inline&joinLevel=abc"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    Ok(())
}
