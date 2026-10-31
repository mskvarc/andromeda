//! A boolean after an ordering operator in q is bad request data.

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
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities refuses a q whose ordering operator is followed by true or false with
/// `BadRequestData`, through the GET parameter and the POST member, and answers the Boolean after ==
/// Clause: 4.9, 5.7.4.4, 6.18.3.2, 6.24.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_qry_3050_a_boolean_after_an_ordering_operator_in_q_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_evolution = TemporalEntity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("b", Property::builder(true).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&sensor_evolution).send().await?.created()?;
    // QueryTerm =/ Attribute Operator ComparableValue: true and false are not ComparableValue
    // The model refuses this q, so the query is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities?type=Sensor_{namespace}&q=b%3Etrue&timerel=after&timeAt=2000-01-01T00%3A00%3A00Z"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses this q, so the query is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities?type=Sensor_{namespace}&q=b%3C%3Dfalse&timerel=after&timeAt=2000-01-01T00%3A00%3A00Z"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses this q, so the query is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities?type=Sensor_{namespace}&q=b%3E%3Dtrue&timerel=after&timeAt=2000-01-01T00%3A00%3A00Z"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses this q, so the query is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("temporal/entities?type=Sensor_{namespace}&q=b%3Cfalse&timerel=after&timeAt=2000-01-01T00%3A00%3A00Z"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_5 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "temporal/entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Sensor_{namespace}\" }} ],\n  \"q\": \"b>true\",\n  \"temporalQ\": {{ \"timerel\": \"after\", \"timeAt\": \"2000-01-01T00:00:00Z\" }}\n}}")));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_6 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "temporal/entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Sensor_{namespace}\" }} ],\n  \"q\": \"b<=false\",\n  \"temporalQ\": {{ \"timerel\": \"after\", \"timeAt\": \"2000-01-01T00:00:00Z\" }}\n}}")));
    let answer_6 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // the Boolean after == is answered
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&sensor_type)
        .q(q::attr("b").eq(true))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    Ok(())
}
