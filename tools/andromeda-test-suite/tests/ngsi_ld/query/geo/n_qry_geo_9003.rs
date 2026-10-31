//! A near distance outside the positive number grammar is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        ErrorType,
        NgsiLdMediaType,
        Property,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: Query Entities with near;maxDistance==500 answers 200, and with a distance spelt +500, .5, 5., 007 or
/// 1,000, or of 0, -5 or `NaN`, answers 400 `BadRequestData` with a title
/// Clause: 4.10, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_geo_9003_a_near_distance_outside_the_positive_number_grammar_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_near_distance_id = namespace.urn("Vehicle", "near-distance")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_near_distance = Entity::builder()
        .id(vehicle_near_distance_id)
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle_near_distance).send().await?.done()?;
    broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .geo_q(geoquery::near_max(500, Shape::Point(Position::try_new(1.0, 2.0)?)))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&geometry=Point&coordinates=%5B1,2%5D&georel=near%3BmaxDistance%3D%3D%2B500"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&geometry=Point&coordinates=%5B1,2%5D&georel=near%3BmaxDistance%3D%3D.5"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&geometry=Point&coordinates=%5B1,2%5D&georel=near%3BmaxDistance%3D%3D5."),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&geometry=Point&coordinates=%5B1,2%5D&georel=near%3BmaxDistance%3D%3D007"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_5 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&geometry=Point&coordinates=%5B1,2%5D&georel=near%3BmaxDistance%3D%3D1%2C000"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_6 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&geometry=Point&coordinates=%5B1,2%5D&georel=near%3BmaxDistance%3D%3D0"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_6 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_7 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&geometry=Point&coordinates=%5B1,2%5D&georel=near%3BmaxDistance%3D%3D-5"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_7 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_7).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_8 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&geometry=Point&coordinates=%5B1,2%5D&georel=near%3BmaxDistance%3D%3DNaN"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer_8 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request_8).await;
    expect_that!(&answer_8, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    Ok(())
}
