//! Creating an entity map with geometry property without a geojson accept is bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{
    HeaderName,
    HeaderValue,
    Method,
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use ngsi_ld::model::{
    Entity,
    ErrorType,
    geometry::{Position, Shape},
};

/// Pins: Create `EntityMap` for Query Entities refuses `geometryProperty` with `BadRequestData` unless the Accept is
/// `application/geo+json`, through the GET binding and the POST binding alike, and creates no `EntityMap`
/// Clause: 5.14.4.4, 5.7.2.4, 6.3.15
#[gtest]
#[tokio::test]
async fn n_map_qen_3001_creating_an_entity_map_with_geometry_property_without_a_geojson_accept_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let entities = [Entity::builder()
        .id(building_id)
        .entity_type(building_type)
        .geo_property("location", Shape::Point(Position::try_new(2.2945, 48.8584)?))
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let entity_map_header = HeaderName::from_static("ngsild-entitymap");
    // 5.14.4.4: "If geometryProperty parameter is present and the Accept Header is not set to application/geo+json, then an error of type BadRequestData shall be raised"
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker,
        Method::GET,
        &format!("entities?type=Building_{namespace}&entityMap=true&geometryProperty=location"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_title()
            .without_header(entity_map_header.clone())
    );
    let request_2 = request_to(&broker, Method::POST, "entityOperations/query?geometryProperty=location")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"entityMap\": true\n}}"
        )));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_title()
            .without_header(entity_map_header.clone())
    );
    let core_context = SuiteConfig::get()?.core_context();
    let request_3 = request_to(&broker, Method::POST, "entityOperations/query?geometryProperty=location")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"))
        .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"))
        .body(Bytes::from(format!(
            "{{\n  \"type\": \"Query\",\n  \"entities\": [ {{ \"type\": \"Building_{namespace}\" }} ],\n  \"entityMap\": true,\n  \"@context\": \"{core_context}\"\n}}"
        )));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_title()
            .without_header(entity_map_header)
    );
    Ok(())
}
