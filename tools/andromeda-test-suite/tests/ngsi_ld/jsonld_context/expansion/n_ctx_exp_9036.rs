//! Every entity write refuses a scoped user context.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    core_context::core_entry,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Attribute, AttributeName, Context, ContextEntry, Entity, EntityFragment, ErrorType, InlineContext, NgsiLdMediaType, Property},
};

/// Pins: Append and Update Entity Attributes, Partial Attribute Update, Replace Attribute, Replace Entity and Merge
/// Entity under a body @context defining gear with a scoped @context each answer 400 `BadRequestData`, and gear
/// keeps its value
/// Clause: 5.5.7, 5.6.2.4, 5.6.3.4, 5.6.4.4, 5.6.17.4, 5.6.18.4, 5.6.19.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9036_every_entity_write_refuses_a_scoped_user_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let device_writes_id = namespace.urn("Device", "writes")?;
    let device_type = namespace.entity_type("Device")?;
    let body_context = broker.with_context(Context::Multiple(vec![core_entry()?, ContextEntry::Inline(format!(r#"{{"gear": {{"@id": "https://example.org/{namespace}/gear", "@context": {{"observedAt": "https://example.org/{namespace}/rewritten"}}}}}}"#).parse::<InlineContext>()?)]))?.with_context_delivery(ContextDelivery::LdJsonBody);
    let device_writes = Entity::builder()
        .id(device_writes_id.clone())
        .entity_type(device_type.clone())
        .property("gear", 1)
        .build()?;
    broker.entities().create(&device_writes).send().await?.done()?;
    let fragment = EntityFragment::builder().property("gear", 2).build()?;
    let answer = body_context
        .with_responses()
        .entities()
        .append_attributes(&device_writes_id, &fragment)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_2 = EntityFragment::builder().property("gear", 2).build()?;
    let answer_2 = body_context
        .with_responses()
        .entities()
        .update_attributes(&device_writes_id, &fragment_2)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let patch = Attribute::from(Property::builder(2).build()?).into_patch();
    let answer_3 = body_context
        .with_responses()
        .entities()
        .update_attribute(&device_writes_id, &"gear".parse::<AttributeName>()?, patch)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let attribute = Attribute::from(Property::builder(2).build()?);
    let answer_4 = body_context
        .with_responses()
        .entities()
        .replace_attribute(&device_writes_id, &"gear".parse::<AttributeName>()?, &attribute)
        .send()
        .await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let device_writes_replacement = Entity::builder()
        .id(device_writes_id.clone())
        .entity_type(device_type.clone())
        .property("gear", 2)
        .build()?;
    let answer_5 = body_context.with_responses().entities().replace(&device_writes_replacement).send().await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragment_3 = EntityFragment::builder().property("gear", 2).build()?;
    let answer_6 = body_context.with_responses().entities().merge(&device_writes_id, &fragment_3).send().await;
    expect_that!(&answer_6, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_writes_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("gear", any_attribute().value(1)));
    Ok(())
}
