//! Post query a boolean after an ordering operator is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::problem,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities by POST refuses a q member whose ordering operator is followed by true or false with
/// `BadRequestData`, under an application/json and an application/ld+json body, and answers the Boolean after ==
/// Clause: 4.9, 5.2.23, 5.7.2.4, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_qlg_3002_post_query_a_boolean_after_an_ordering_operator_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let core = config.core_context();
    let sensor = Entity::builder()
        .id(sensor_id.clone())
        .entity_type(sensor_type.clone())
        .property("b", true)
        .build()?;
    broker.entities().create(&sensor).send().await?.done()?;
    // Table 5.2.23-1: q is "A valid query string as per clause 4.9", and ComparableValue excludes true and false
    // The model refuses a member of this document, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Sensor_{namespace}\" }} ],\n  \"q\": \"b>true\"\n}}"
        )));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_2 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Sensor_{namespace}\" }} ],\n  \"q\": \"b>=false\"\n}}"
        )));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_3 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Sensor_{namespace}\" }} ],\n  \"q\": \"b<true\"\n}}"
        )));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_4 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Sensor_{namespace}\" }} ],\n  \"q\": \"b<=false\"\n}}"
        )));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_5 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Sensor_{namespace}\" }} ],\n  \"q\": \"b>true\",\n  \"@context\": \"{core}\"\n}}"
        )));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // the Boolean after == is well-formed
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&sensor_type)
        .q(q::attr("b").eq(true))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&sensor_id)));
    Ok(())
}
