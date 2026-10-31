//! Only the four content inputs narrow a registration discovery.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{
        ErrorType,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::{geoquery, q},
};

/// Pins: Query Context Source Registrations answers 200 when narrowed by only a type, attrs, q or a geo-query, and
/// 400 `BadRequestData` (too wide) with only id, `idPattern`, `scopeQ`, id with `idPattern`, or nothing
/// Clause: 5.10.2.4
#[gtest]
#[tokio::test]
async fn n_csr_qry_9004_only_the_four_content_inputs_narrow_a_registration_discovery() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(namespace.entity_type("Vehicle")?)
        .send()
        .await?;
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .attrs(vec![format!("speed_{namespace}").parse()?])
        .send()
        .await?;
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .q(q::attr(&namespace.attribute("speed")?).gt(10))
        .send()
        .await?;
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .geo_q(geoquery::near_max(100, Shape::Point(Position::try_new(1.0, 2.0)?)))
        .send()
        .await?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("csourceRegistrations?id=urn%3Angsi-ld%3AVehicle%3A{namespace}-too-wide"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("csourceRegistrations?idPattern=urn%3Angsi-ld%3AVehicle%3A{namespace}-.%2A"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        "csourceRegistrations?scopeQ=%2FMadrid",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("csourceRegistrations?id=urn%3Angsi-ld%3AVehicle%3A{namespace}-too-wide&idPattern=urn%3Angsi-ld%3AVehicle%3A{namespace}-.%2A"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_5 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, "csourceRegistrations")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
