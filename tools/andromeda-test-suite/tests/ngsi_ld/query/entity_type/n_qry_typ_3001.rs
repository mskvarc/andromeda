//! A type selection outside the grammar is bad request data in every get shape.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::type_selection,
};

/// Pins: Query Entities by GET refuses a type selection that does not derive from the 4.17 grammar (unbalanced or
/// empty parentheses, a dangling or doubled operator, an or inside parentheses, a conjunction left of an operator
/// without parentheses) with `BadRequestData`, and answers the valid forms beside them
/// Clause: 4.17, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_typ_3001_a_type_selection_outside_the_grammar_is_bad_request_data_in_every_get_shape() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let thing_id = namespace.urn("Thing", 1)?;
    let thing_2_id = namespace.urn("Thing", 2)?;
    let motorhome_type = namespace.entity_type("Motorhome")?;
    let entities = [
        Entity::builder()
            .id(thing_id)
            .entity_type(vec![namespace.entity_type("Home")?, namespace.entity_type("Vehicle")?])
            .build()?,
        Entity::builder().id(thing_2_id.clone()).entity_type(motorhome_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // 4.17: EntityTypes = OrEntityType *(orOp OrEntityType), OrEntityType = "(" EntityType *(andOp EntityType) ")" / EntityType; an unclosed parenthesis
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=%28Home_{namespace}%3BVehicle_{namespace}"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // an unopened parenthesis
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_2 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Home_{namespace}%3BVehicle_{namespace}%29"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // empty parentheses
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_3 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=%28%29%2CHome_{namespace}"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_3 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // a conjunction with a missing operand
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_4 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=%28Home_{namespace}%3B%29"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_4 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_5 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=%28%3BHome_{namespace}%29"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_5 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // a dangling or doubled disjunction operator, in both spellings
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_6 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Home_{namespace}%7C"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_6 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_6).await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_7 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Home_{namespace}%2C"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_7 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_7).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_8 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Home_{namespace}%7C%7CVehicle_{namespace}"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_8 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_8).await;
    expect_that!(&answer_8, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_9 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Home_{namespace}%2C%2CVehicle_{namespace}"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_9 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_9).await;
    expect_that!(&answer_9, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // an or inside the parentheses: only andOp may join the types of a conjunction
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_10 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=%28Home_{namespace}%7CVehicle_{namespace}%29"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_10 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_10).await;
    expect_that!(&answer_10, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // a parenthesised conjunction joined to a bare type with an and operator
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_11 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=%28Home_{namespace}%3BVehicle_{namespace}%29%3BMotorhome_{namespace}"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_11 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_11).await;
    expect_that!(&answer_11, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // the refusal is the same when the selection creates an EntityMap
    // The client cannot write a parameter of this read, so it is sent as written.
    let request_12 = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Home_{namespace}%3BVehicle_{namespace}&entityMap=true"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_12 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request_12).await;
    expect_that!(
        &answer_12,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_title()
            .without_header(HeaderName::from_static("ngsild-entitymap"))
    );
    // the valid forms: one type in parentheses, a conjunction joined by a comma to a bare type (4.17 EXAMPLE 3, alternative representation), a disjunction with the pipe
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&motorhome_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&thing_2_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(type_selection::all_of([&namespace.entity_type("Home")?, &namespace.entity_type("Vehicle")?]).or(&motorhome_type))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(type_selection::all_of([&namespace.entity_type("Home")?, &namespace.entity_type("Vehicle")?]).or(&motorhome_type))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    Ok(())
}
