//! A json batch delete with a context link deletes the listed entities.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType},
};

/// Pins: an application/json Batch Entity Delete carrying a context Link takes its @context from that Link, answers
/// 204 and deletes every listed entity
/// Clause: 6.3.5, 5.6.10.4, 6.17.3.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0010_a_json_batch_delete_with_a_context_link_deletes_the_listed_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Building_{namespace}": "https://test.andromeda/{namespace}/Building"}}"#).parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    let entities = [
        Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_2_id.clone()).entity_type(building_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let ids = [building_id.clone(), building_2_id.clone()];
    linked.entities().batch_delete(&ids).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_2_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
