//! Id or type in attrs is refused by every operation.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: attrs=speed,id and attrs=speed,type on Query Entities, Retrieve Entity, Query Temporal Evolution of
/// Entities and Retrieve Temporal Evolution of an Entity each answer 400 `BadRequestData` naming the member
/// Clause: Tables 6.4.3.2-1, 6.5.3.1-1, 6.18.3.2-1, 6.19.3.1-1
#[gtest]
#[tokio::test]
async fn n_qry_prj_9002_id_or_type_in_attrs_is_refused_by_every_operation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .scope(["/Madrid/Centro"])
        .property("speed", Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // Query Entities.
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&id=urn%3Angsi-ld%3AVehicle%3A{namespace}-1&attrs=speed%2Cid"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("id")
    );
    // Retrieve Entity.
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities/{vehicle_id}?attrs=speed%2Cid"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("id")
    );
    // Query Temporal Evolution of Entities.
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("temporal/entities?type=Vehicle_{namespace}&id=urn%3Angsi-ld%3AVehicle%3A{namespace}-1&attrs=speed%2Cid&timerel=after&timeAt=2000-01-01T00%3A00%3A00Z"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("id")
    );
    // Retrieve Temporal Evolution of an Entity.
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities/{vehicle_id}?attrs=speed%2Cid"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(
        &answer_4,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("id")
    );
    // Query Entities.
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_5 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&id=urn%3Angsi-ld%3AVehicle%3A{namespace}-1&attrs=speed%2Ctype"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_5).await;
    expect_that!(
        &answer_5,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("type")
    );
    // Retrieve Entity.
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_6 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities/{vehicle_id}?attrs=speed%2Ctype"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_6 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_6).await;
    expect_that!(
        &answer_6,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("type")
    );
    // Query Temporal Evolution of Entities.
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_7 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::GET, &format!("temporal/entities?type=Vehicle_{namespace}&id=urn%3Angsi-ld%3AVehicle%3A{namespace}-1&attrs=speed%2Ctype&timerel=after&timeAt=2000-01-01T00%3A00%3A00Z"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_7 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_7).await;
    expect_that!(
        &answer_7,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("type")
    );
    // Retrieve Temporal Evolution of an Entity.
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_8 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities/{vehicle_id}?attrs=speed%2Ctype"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_8 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_8).await;
    expect_that!(
        &answer_8,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("type")
    );
    Ok(())
}
