//! Entity map lifetime without entity map is invalid request.

use andromeda_test_suite::{
    broker,
    header_matcher::present,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    received_status::ExpectedStatus,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Retrieve Entity and Query Entities with `entityMapLifetime` PT1H and no `entityMap`, or `entityMap` false,
/// answer 400 `InvalidRequest` without an `NGSILD-EntityMap` header, while `entityMap` true with the lifetime
/// answers with one
/// Clause: 6.3.20, Table 6.4.3.2-1, Table 6.5.3.1-1, 5.14.4
#[gtest]
#[tokio::test]
async fn n_map_qen_9010_entity_map_lifetime_without_entity_map_is_invalid_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities/{vehicle_id}?entityMapLifetime=PT1H"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest)
            .with_detail_containing("entityMapLifetime")
            .without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&entityMapLifetime=PT1H"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest).without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&entityMap=false&entityMapLifetime=PT1H"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let entity_map_request = EntityMapRequest::returned_for("PT1H".parse()?);
    let page = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.entity_map(), present());
    Ok(())
}
