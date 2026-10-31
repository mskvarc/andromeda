//! An id pattern that is not a regular expression is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::IdPattern,
};

/// Pins: an `idPattern` that is not a valid regular expression (a doubled quantifier, an unclosed bracket) is
/// refused with 400 `BadRequestData`
/// Clause: 5.7.2.4, 6.4.3.2, 5.5.2
/// Recorded: 5.7.2.4 lists invalid ids, q, geoquery and csf as `BadRequestData` but not an invalid `idPattern`;
/// Andromeda refuses it as `BadRequestData` (clause 5.5.2: input data that does not meet the requirements of the
/// operation)
#[gtest]
#[tokio::test]
async fn o_qry_ent_0005_an_id_pattern_that_is_not_a_regular_expression_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let entities = [
        Entity::builder().id(building_id).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_2_id).entity_type(building_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .id_pattern("invalid_entity_id_pattern**".parse::<IdPattern>()?)
        .entity_type(&building_type)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &broker.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        &format!("entities?type=Vehicle_{namespace}&idPattern=%5B"),
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer_2 = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
