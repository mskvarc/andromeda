//! An unsupported temporal representation is an invalid request.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Retrieve and Query Temporal Evolution with a format or options value Table 6.3.12-1 does not define, an
/// entity rendering, both temporal renderings at once, or an unknown token beside a supported one, answer 400
/// `InvalidRequest` with a title
/// Clause: 6.3.20, Table 6.3.12-1
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9005_an_unsupported_temporal_representation_is_an_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_temporal_representation_id = namespace.urn("Vehicle", "temporal-representation")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_temporal_representation = Entity::builder()
        .id(vehicle_temporal_representation_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_temporal_representation).send().await?.done()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities/{vehicle_temporal_representation_id}?timerel=after&timeAt=2000-01-01T00:00:00Z&format=invalidFormat"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities?type=Vehicle_{namespace}&timerel=after&timeAt=2000-01-01T00:00:00Z&format=invalidFormat"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities/{vehicle_temporal_representation_id}?timerel=after&timeAt=2000-01-01T00:00:00Z&format=normalized"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities?type=Vehicle_{namespace}&timerel=after&timeAt=2000-01-01T00:00:00Z&format=normalized"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_5 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities/{vehicle_temporal_representation_id}?timerel=after&timeAt=2000-01-01T00:00:00Z&format=keyValues"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_6 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities?type=Vehicle_{namespace}&timerel=after&timeAt=2000-01-01T00:00:00Z&format=keyValues"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_6 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_7 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities/{vehicle_temporal_representation_id}?timerel=after&timeAt=2000-01-01T00:00:00Z&format=concise"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_7 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_7).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_8 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities?type=Vehicle_{namespace}&timerel=after&timeAt=2000-01-01T00:00:00Z&format=concise"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_8 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_8).await;
    expect_that!(&answer_8, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_9 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities/{vehicle_temporal_representation_id}?timerel=after&timeAt=2000-01-01T00:00:00Z&options=invalidOptions"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_9 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_9).await;
    expect_that!(&answer_9, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_10 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities?type=Vehicle_{namespace}&timerel=after&timeAt=2000-01-01T00:00:00Z&options=invalidOptions"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_10 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_10).await;
    expect_that!(&answer_10, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_11 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities/{vehicle_temporal_representation_id}?timerel=after&timeAt=2000-01-01T00:00:00Z&options=temporalValues,invalidOptions"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_11 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_11).await;
    expect_that!(&answer_11, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_12 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities?type=Vehicle_{namespace}&timerel=after&timeAt=2000-01-01T00:00:00Z&options=temporalValues,invalidOptions"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_12 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_12).await;
    expect_that!(&answer_12, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_13 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities/{vehicle_temporal_representation_id}?timerel=after&timeAt=2000-01-01T00:00:00Z&options=keyValues"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_13 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_13).await;
    expect_that!(&answer_13, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_14 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities?type=Vehicle_{namespace}&timerel=after&timeAt=2000-01-01T00:00:00Z&options=keyValues"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_14 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_14).await;
    expect_that!(&answer_14, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_15 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities/{vehicle_temporal_representation_id}?timerel=after&timeAt=2000-01-01T00:00:00Z&options=temporalValues,aggregatedValues"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_15 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_15).await;
    expect_that!(&answer_15, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_16 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("temporal/entities?type=Vehicle_{namespace}&timerel=after&timeAt=2000-01-01T00:00:00Z&options=temporalValues,aggregatedValues"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_16 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_16).await;
    expect_that!(&answer_16, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_17 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)), Method::GET, &format!("temporal/entities/{vehicle_temporal_representation_id}?timerel=after&timeAt=2000-01-01T00:00:00Z&format=temporalValues&options=aggregatedValues,invalidOptions"))?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_17 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_17).await;
    expect_that!(&answer_17, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_18 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!(
            "temporal/entities?type=Vehicle_{namespace}&timerel=after&timeAt=2000-01-01T00:00:00Z&format=temporalValues&options=aggregatedValues,invalidOptions"
        ),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_18 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_18).await;
    expect_that!(&answer_18, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).with_title());
    Ok(())
}
