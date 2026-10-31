//! A type selection outside the grammar is bad request data in a post entity selector.

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
    query::type_selection,
};

/// Pins: Query Entities by POST refuses an `EntitySelector` whose type is a type selection outside the 4.17 grammar
/// with `BadRequestData`, in the first and in a later selector of a disjunction, and answers a conjunction written
/// with parentheses
/// Clause: 4.17, 5.2.33, 5.7.2.4, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_qry_typ_3002_a_type_selection_outside_the_grammar_is_bad_request_data_in_a_post_entity_selector() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let thing_id = namespace.urn("Thing", 1)?;
    let core = config.core_context();
    let entities = [Entity::builder()
        .id(thing_id.clone())
        .entity_type(vec![namespace.entity_type("Home")?, namespace.entity_type("Vehicle")?])
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // Table 5.2.33-1: type is "A valid type selection string as per clause 4.17"
    // The model refuses a member of this document, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Home_{namespace};Vehicle_{namespace}\" }} ]\n}}"
        )));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_2 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"((Home_{namespace};Vehicle_{namespace}))\" }} ]\n}}"
        )));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_3 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"(Home_{namespace};Vehicle_{namespace}\" }} ]\n}}"
        )));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_4 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"(Home_{namespace}|Vehicle_{namespace})\" }} ]\n}}"
        )));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // a later selector of the disjunction is as binding as the first
    // The model refuses a member of this document, so it is sent as written.
    let request_5 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n  \"type\": \"Query\",\n  \"entities\": [\n    {{ \"type\": \"(Home_{namespace};Vehicle_{namespace})\" }},\n    {{ \"type\": \"Home_{namespace};Vehicle_{namespace}\" }}\n  ]\n}}")));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The model refuses a member of this document, so it is sent as written.
    let request_6 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Home_{namespace};Vehicle_{namespace}\" }} ],\n  \"@context\": \"{core}\"\n}}"
        )));
    let answer_6 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // the parenthesised conjunction is answered
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(type_selection::all_of([&namespace.entity_type("Home")?, &namespace.entity_type("Vehicle")?]).build()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&thing_id)));
    Ok(())
}
