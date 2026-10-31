//! Creating an entity map restricted only by a pick is too wide.

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
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::projection::Projection,
};

/// Pins: Create `EntityMap` for Query Entities whose only restriction is a pick is too wide and refused with
/// `BadRequestData` through the GET and the POST binding, and creates no `EntityMap`
/// Clause: 5.14.4.4, 5.7.2.4, 4.21
#[gtest]
#[tokio::test]
async fn n_map_qen_3030_creating_an_entity_map_restricted_only_by_a_pick_is_too_wide() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let entities = [Entity::builder().id(building_id).entity_type(building_type.clone()).property("p", 1).build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // 5.14.4.4 repeats the narrowing inputs of 5.7.2.4: a projection is none of them
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        "entities?pick=p&entityMap=true",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_title()
            .without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    // The client refuses the unbounded query before sending it, so the document is sent as written.
    let request_2 = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from("{\n  \"type\": \"Query\",\n  \"pick\": [ \"p\" ],\n  \"entityMap\": true\n}"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_title()
            .without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    // control: the same pick beside a type selector creates the EntityMap
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query_projected()
        .entity_type(&building_type)
        .pick("p".parse::<Projection>()?)
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.entity_map(), present());
    expect_that!(page.items(), len(eq(1)));
    Ok(())
}
