//! Creating an entity map through a query body with an empty array is bad request data.

use andromeda_test_suite::{
    broker,
    header_matcher::present,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    received_status::ExpectedStatus,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderName,
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{EntityMapRequest, QueryMethod},
    },
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::projection::Projection,
};

/// Pins: Create `EntityMap` for Query Entities through the POST binding refuses a Query body whose entities, attrs,
/// pick, omit or ordering member breaks its data type restriction, and creates no `EntityMap`
/// Clause: 5.2.23, 5.2.43, 5.14.4.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_map_qen_3010_creating_an_entity_map_through_a_query_body_with_an_empty_array_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let entities = [Entity::builder()
        .id(building_id)
        .entity_type(building_type.clone())
        .property("height", 5)
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // Table 5.2.23-1: "Empty array (0 length) is not allowed"
    // The client writes no empty list of entity selectors, so the document is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(
            "{\n  \"type\": \"Query\",\n  \"entities\": [],\n  \"attrs\": [ \"height\" ],\n  \"entityMap\": true\n}",
        ));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_title()
            .without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    // The model refuses a member of this document, so it is sent as written.
    let request_2 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"pick\": [],\n  \"entityMap\": true\n}}"
        )));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_title()
            .without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    // The model refuses a member of this document, so it is sent as written.
    let request_3 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"omit\": [],\n  \"entityMap\": true\n}}"
        )));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_title()
            .without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    // The model refuses a member of this document, so it is sent as written.
    let request_4 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"attrs\": [],\n  \"entityMap\": true\n}}"
        )));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(
        &answer_4,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_title()
            .without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    // Table 5.2.43-1: orderBy has cardinality 1
    // The model refuses a member of this document, so it is sent as written.
    let request_5 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query?local=true")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"ordering\": {{ \"collation\": \"und-x-icu\" }},\n  \"entityMap\": true\n}}")));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_5).await;
    expect_that!(
        &answer_5,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_title()
            .without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    // control: the same body without the fault creates the EntityMap
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .pick("id".parse::<Projection>()?)
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.entity_map(), present());
    expect_that!(page.items(), len(eq(1)));
    Ok(())
}
