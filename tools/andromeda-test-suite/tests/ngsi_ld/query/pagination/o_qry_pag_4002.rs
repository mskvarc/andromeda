//! Following a next pointer without the context the walk started with is bad request data.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType},
};

/// Pins: following the rel="next" pointer of a page that was fetched with a user @context, with no @context at all
/// and so with the core @context alone, is refused with `BadRequestData`
/// Clause: 5.5.9.1, 6.3.10
/// Recorded: clause 5.5.9.1 says an attempt to use a different @context during pagination "should result in an
/// `BadRequestData` error"; a "should", so the file is kind O; a request that supplies no @context is read with the
/// core @context, which is not the one the walk started with
#[gtest]
#[tokio::test]
async fn o_qry_pag_4002_following_a_next_pointer_without_the_context_the_walk_started_with_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/name"}}"#)
            .parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    let body_context = broker
        .with_context(Context::Single(ctx.entry()?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let entities = [
        Entity::builder()
            .id(building_id)
            .entity_type(building_type.clone())
            .property("name", "b1")
            .build()?,
        Entity::builder()
            .id(building_2_id)
            .entity_type(building_type.clone())
            .property("name", "b2")
            .build()?,
        Entity::builder()
            .id(building_3_id)
            .entity_type(building_type.clone())
            .property("name", "b3")
            .build()?,
    ];
    body_context.entities().batch_create(&entities).send().await?.done()?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .entity_type(&building_type)
        .limit(1)
        .send()
        .await?;
    expect_that!(page.next(), some(anything()));
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page.next_page().or_fail()?)
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
