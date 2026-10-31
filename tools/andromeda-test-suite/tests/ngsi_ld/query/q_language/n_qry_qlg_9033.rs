//! A malformed escape is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Query Entities whose quoted string holds \p, a truncated \u00, the non-hexadecimal \uZZZZ, a lone high
/// surrogate, a lone low surrogate or a high surrogate followed by \u0041 answers 400 `BadRequestData`
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9033_a_malformed_escape_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let escape_malformed_id = namespace.urn("Escape", "malformed")?;
    let escape_type = namespace.entity_type("Escape")?;
    let escape_malformed = Entity::builder()
        .id(escape_malformed_id)
        .entity_type(escape_type)
        .property("name", "anything")
        .build()?;
    broker.entities().create(&escape_malformed).send().await?.done()?;
    // The model refuses this q, so the query is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Escape_{namespace}&id=urn%3Angsi-ld%3AEscape%3A{namespace}-malformed&q=name%3D%3D%22a%5Cp%22"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this q, so the query is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Escape_{namespace}&id=urn%3Angsi-ld%3AEscape%3A{namespace}-malformed&q=name%3D%3D%22a%5Cu00%22"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this q, so the query is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Escape_{namespace}&id=urn%3Angsi-ld%3AEscape%3A{namespace}-malformed&q=name%3D%3D%22a%5CuZZZZ%22"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this q, so the query is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Escape_{namespace}&id=urn%3Angsi-ld%3AEscape%3A{namespace}-malformed&q=name%3D%3D%22a%5Cud834%22"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this q, so the query is sent as written.
    let request_5 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Escape_{namespace}&id=urn%3Angsi-ld%3AEscape%3A{namespace}-malformed&q=name%3D%3D%22a%5Cudd1e%22"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The model refuses this q, so the query is sent as written.
    let request_6 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Escape_{namespace}&id=urn%3Angsi-ld%3AEscape%3A{namespace}-malformed&q=name%3D%3D%22a%5Cud834%5Cu0041%22"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_6 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
