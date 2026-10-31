//! A construct only one engine supports is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::{Locality, q},
};

/// Pins: Query Entities with a q pattern holding a Unicode property class, an end-of-text or hex escape, a scoped
/// or cleared inline option, a negated class name or an ambiguous range answers 400 `BadRequestData`
/// Clause: 4.9
/// Recorded: IEEE 1003.2 leaves a backslash before an ordinary character, an inline option, a repeated duplication
/// symbol, an undefined class name and a range outside the POSIX locale undefined or unspecified; Andromeda refuses
/// each with `BadRequestData` so that the stored-query and the subscription evaluators never read one pattern two
/// ways
#[gtest]
#[tokio::test]
async fn a_qry_qlg_9022_a_construct_only_one_engine_supports_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_dialect_b_id = namespace.urn("Building", "dialect-b")?;
    let building_type = namespace.entity_type("Building")?;
    let building_dialect_b = Entity::builder()
        .id(building_dialect_b_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Mercedes")
        .build()?;
    broker.entities().create(&building_dialect_b).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_dialect_b_id.clone()])
        .q(q::attr("name").matches("\"\\p{L}\""))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_dialect_b_id.clone()])
        .q(q::attr("name").matches("\"\\P{L}\""))
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_dialect_b_id.clone()])
        .q(q::attr("name").matches("\"a\\z\""))
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_dialect_b_id.clone()])
        .q(q::attr("name").matches("\"\\x{61}\""))
        .send()
        .await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_dialect_b_id.clone()])
        .q(q::attr("name").matches("\"(?m:ab)\""))
        .send()
        .await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_dialect_b_id.clone()])
        .q(q::attr("name").matches("\"(?u)a\""))
        .send()
        .await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_7 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_dialect_b_id.clone()])
        .q(q::attr("name").matches("\"(?U)a+\""))
        .send()
        .await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_dialect_b_id.clone()])
        .q(q::attr("name").matches("\"(?-s)a\""))
        .send()
        .await;
    expect_that!(&answer_8, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_9 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_dialect_b_id.clone()])
        .q(q::attr("name").matches("\"Merc\\x33de\""))
        .send()
        .await;
    expect_that!(&answer_9, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_10 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_dialect_b_id.clone()])
        .q(q::attr("name").matches("\"[[:^alpha:]]\""))
        .send()
        .await;
    expect_that!(&answer_10, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_11 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_dialect_b_id.clone()])
        .q(q::attr("name").matches("\"[a-b-c]\""))
        .send()
        .await;
    expect_that!(&answer_11, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_12 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_dialect_b_id.clone()])
        .q(q::attr("name").matches("\"[[:alpha:]-z]\""))
        .send()
        .await;
    expect_that!(&answer_12, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
