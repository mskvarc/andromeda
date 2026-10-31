//! A scope level past a thousand characters is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem, spelling_matcher::alone};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, ErrorType, NgsiLdMediaType, OneOrMany, Scope, ScopeChange, Value},
    query::scope_query::ScopeQuery,
};

/// Pins: A Scope level of 1,001 characters is refused with 400 `BadRequestData` by Create Entity, Append and Update
/// Entity Attributes, Merge Entity and a `scopeQ`, and the stored scope /a/b/c stands
/// Clause: 4.18, 4.19
/// Recorded: Andromeda limit, not v1.9.1: the 4.18 grammar sets no length; Andromeda stores each Scope level in an
/// index counting at most 1,000 characters and a whole Scope in an index row of bounded width, and refuses one past
/// either bound
#[gtest]
#[tokio::test]
async fn a_cmn_val_9025_a_scope_level_past_a_thousand_characters_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_long_level_create_id = namespace.urn("Building", "long-level-create")?;
    let building_type = namespace.entity_type("Building")?;
    let building_long_level_id = namespace.urn("Building", "long-level")?;
    let building_long_level_create = Entity::builder()
        .id(building_long_level_create_id)
        .entity_type(building_type.clone())
        .scope(format!("/{}", "a".repeat(1001)).as_str())
        .build()?;
    let answer = broker.with_responses().entities().create(&building_long_level_create).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let building_long_level = Entity::builder()
        .id(building_long_level_id.clone())
        .entity_type(building_type.clone())
        .scope("/a/b/c")
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_long_level).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one(format!("/{}", "a".repeat(1001)).as_str().parse::<Scope>()?)))
        .build()?;
    let answer_2 = broker
        .with_responses()
        .entities()
        .append_attributes(&building_long_level_id, &fragment)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_2 = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one(format!("/{}", "a".repeat(1001)).as_str().parse::<Scope>()?)))
        .build()?;
    let answer_3 = broker
        .with_responses()
        .entities()
        .update_attributes(&building_long_level_id, &fragment_2)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_3 = EntityFragment::builder()
        .scope(ScopeChange::Set(OneOrMany::one(format!("/{}", "a".repeat(1001)).as_str().parse::<Scope>()?)))
        .build()?;
    let answer_4 = broker.with_responses().entities().merge(&building_long_level_id, &fragment_3).send().await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_long_level_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(alone(&"/a/b/c".parse::<Scope>()?)));
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .scope_q(format!("/{}", "a".repeat(1001)).as_str().parse::<ScopeQuery>()?)
        .send()
        .await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
