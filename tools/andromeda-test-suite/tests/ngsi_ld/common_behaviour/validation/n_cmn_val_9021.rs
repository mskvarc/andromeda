//! A parameter the operation does not define is an invalid request.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Retrieve Entity, Query Entities, Retrieve and Query Temporal Evolution, Query Context Source Registrations
/// and Query Subscriptions with a URL parameter none of them defines answer 400 `InvalidRequest` with a title
/// Clause: 6.3.20
#[gtest]
#[tokio::test]
async fn n_cmn_val_9021_a_parameter_the_operation_does_not_define_is_an_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_unknown_parameter_id = namespace.urn("Vehicle", "unknown-parameter")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_unknown_parameter = Entity::builder()
        .id(vehicle_unknown_parameter_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_unknown_parameter).send().await?.done()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("entities/{vehicle_unknown_parameter_id}?bogusParameter=1"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("entities/{vehicle_unknown_parameter_id}?attrs=speed&bogusParameter=1"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&bogusParameter=1"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities/{vehicle_unknown_parameter_id}?timerel=after&timeAt=2000-01-01T00:00:00Z&bogusParameter=1"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_5 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities?type=Vehicle_{namespace}&timerel=after&timeAt=2000-01-01T00:00:00Z&bogusParameter=1"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_6 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("csourceRegistrations?type=Vehicle_{namespace}&bogusParameter=1"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_6 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_7 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        "subscriptions?bogusParameter=1",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_7 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_7).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    Ok(())
}
