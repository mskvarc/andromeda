//! A query body q with a hop in the grammar form selects the root.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    linked_entity::linked_entities,
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
    query::{Join, q},
};

/// Pins: Query Entities by POST `/entityOperations/query` with a q member in the grammar's Linked Entity form
/// (isParkedIn{category}=="commercial", the comparison after the closing brace) is answered and selects the root
/// whose Linked Entity matches, with join inline and with join flat, while the form with the comparison inside the
/// braces is `BadRequestData`
/// Clause: 4.9, 5.2.8, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_5001_a_query_body_q_with_a_hop_in_the_grammar_form_selects_the_root() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_commercial_id = namespace.urn("Building", "commercial")?;
    let building_type = namespace.entity_type("Building")?;
    let building_residential_id = namespace.urn("Building", "residential")?;
    let car_id = namespace.urn("Car", 1)?;
    let car_type = namespace.entity_type("Car")?;
    let car_2_id = namespace.urn("Car", 2)?;
    let entities = [
        Entity::builder()
            .id(building_commercial_id.clone())
            .entity_type(building_type.clone())
            .property("category", "commercial")
            .build()?,
        Entity::builder()
            .id(building_residential_id.clone())
            .entity_type(building_type.clone())
            .property("category", "residential")
            .build()?,
        Entity::builder()
            .id(car_id.clone())
            .entity_type(car_type.clone())
            .relationship("isParkedIn", building_commercial_id.clone())
            .build()?,
        Entity::builder()
            .id(car_2_id.clone())
            .entity_type(car_type.clone())
            .relationship("isParkedIn", building_residential_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("category").eq("commercial"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let item = page.items().first().or_fail()?;
    expect_that!(linked_entities(item, &["isParkedIn"]), nth(0, has_id(&building_commercial_id)));
    expect_that!(page.items(), nth(0, has_id(&car_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("category").eq("residential"))
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&car_2_id)));
    expect_that!(page_2.items(), nth(1, has_id(&building_residential_id)));
    // The model refuses a member of this document, so it is sent as written.
    let request = request_to(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), Method::POST, "entityOperations/query")?.append_header(ACCEPT, HeaderValue::from_static("application/json")).append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"type\": \"Query\",\n    \"entities\": [\n        {{\n            \"type\": \"Car_{namespace}\"\n        }}\n    ],\n    \"q\": \"isParkedIn{{category==\\\"commercial\\\"}}\",\n    \"join\": \"inline\",\n    \"joinLevel\": 1\n}}")));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
