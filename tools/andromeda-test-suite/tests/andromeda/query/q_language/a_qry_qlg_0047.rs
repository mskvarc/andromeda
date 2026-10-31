//! Scoping or clearing an option other than case insensitivity is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::{IdPattern, q},
};

/// Pins: a q pattern or an `idPattern` that scopes the m, s or x option to a group or clears one of them answers
/// 400 `BadRequestData`; only i may be scoped or cleared
/// Clause: 4.9, 5.7.2.4
/// Recorded: clause 4.9 hands `RegExp` to IEEE 1003.2, which has no inline options; Andromeda admits m, s and x
/// only set for the rest of a pattern, and i also scoped and cleared, and refuses the rest as `BadRequestData`
#[gtest]
#[tokio::test]
async fn a_qry_qlg_0047_scoping_or_clearing_an_option_other_than_case_insensitivity_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let bee_0_id = namespace.urn("Bee", "0")?;
    let bee_type = namespace.entity_type("Bee")?;
    let bee_id = namespace.urn("Bee", 1)?;
    let bee_2_id = namespace.urn("Bee", 2)?;
    let bee_3_id = namespace.urn("Bee", 3)?;
    let bee_4_id = namespace.urn("Bee", 4)?;
    let bee_5_id = namespace.urn("Bee", 5)?;
    let entities = [
        Entity::builder()
            .id(bee_0_id)
            .entity_type(bee_type.clone())
            .property("name", "čebela")
            .build()?,
        Entity::builder().id(bee_id).entity_type(bee_type.clone()).property("name", "Čebela").build()?,
        Entity::builder()
            .id(bee_2_id)
            .entity_type(bee_type.clone())
            .property("name", "ČEBELA")
            .build()?,
        Entity::builder()
            .id(bee_3_id)
            .entity_type(bee_type.clone())
            .property("name", "ČEbela")
            .build()?,
        Entity::builder()
            .id(bee_4_id)
            .entity_type(bee_type.clone())
            .property("name", "cebela")
            .build()?,
        Entity::builder()
            .id(bee_5_id)
            .entity_type(bee_type.clone())
            .property("name", "Škofja Loka")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&bee_type)
        .q(q::attr("name").matches("\"(?m:^a)\""))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&bee_type)
        .id_pattern("(?m:^a)".parse::<IdPattern>()?)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&bee_type)
        .q(q::attr("name").matches("\"(?s:a.b)\""))
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&bee_type)
        .id_pattern("(?s:a.b)".parse::<IdPattern>()?)
        .send()
        .await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&bee_type)
        .q(q::attr("name").matches("\"(?x:a b)\""))
        .send()
        .await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&bee_type)
        .id_pattern("(?x:a b)".parse::<IdPattern>()?)
        .send()
        .await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_7 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&bee_type)
        .q(q::attr("name").matches("\"(?-s)a.b\""))
        .send()
        .await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&bee_type)
        .id_pattern("(?-s)a.b".parse::<IdPattern>()?)
        .send()
        .await;
    expect_that!(&answer_8, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_9 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&bee_type)
        .q(q::attr("name").matches("\"(?i-m)a\""))
        .send()
        .await;
    expect_that!(&answer_9, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_10 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&bee_type)
        .id_pattern("(?i-m)a".parse::<IdPattern>()?)
        .send()
        .await;
    expect_that!(&answer_10, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_11 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&bee_type)
        .q(q::attr("name").matches("\"(?-ix)a\""))
        .send()
        .await;
    expect_that!(&answer_11, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_12 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&bee_type)
        .id_pattern("(?-ix)a".parse::<IdPattern>()?)
        .send()
        .await;
    expect_that!(&answer_12, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
