//! A type selection outside the grammar is bad request data in get and post.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{temporal_query, type_selection},
};

/// Pins: Query Temporal Evolution of Entities refuses a type selection outside the 4.17 grammar with
/// `BadRequestData` through the GET type parameter and the POST `EntitySelector`, and answers the parenthesised
/// conjunction
/// Clause: 4.17, 5.7.4.4, 6.18.3.2, 6.24.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_qry_3040_a_type_selection_outside_the_grammar_is_bad_request_data_in_get_and_post() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let thing_id = namespace.urn("Thing", 1)?;
    let thing_evolution = TemporalEntity::builder()
        .id(thing_id.clone())
        .entity_type(vec![namespace.entity_type("Home")?, namespace.entity_type("Vehicle")?])
        .property("power", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&thing_evolution).send().await?.created()?;
    // 4.17: "For logical and grouping parenthesis are needed"
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities?type=Home_{namespace}%3BVehicle_{namespace}&timerel=after&timeAt=2000-01-01T00%3A00%3A00Z"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities?type=%28%28Home_{namespace}%3BVehicle_{namespace}%29%29&timerel=after&timeAt=2000-01-01T00%3A00%3A00Z"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities?type=%28Home_{namespace}%7CVehicle_{namespace}%29&timerel=after&timeAt=2000-01-01T00%3A00%3A00Z"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_4 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "temporal/entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Home_{namespace};Vehicle_{namespace}\" }} ],\n  \"temporalQ\": {{ \"timerel\": \"after\", \"timeAt\": \"2000-01-01T00:00:00Z\" }}\n}}")));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_5 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "temporal/entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"((Home_{namespace};Vehicle_{namespace}))\" }} ],\n  \"temporalQ\": {{ \"timerel\": \"after\", \"timeAt\": \"2000-01-01T00:00:00Z\" }}\n}}")));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // control: the parenthesised conjunction is answered
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(type_selection::all_of([&namespace.entity_type("Home")?, &namespace.entity_type("Vehicle")?]).build()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&thing_id)));
    Ok(())
}
