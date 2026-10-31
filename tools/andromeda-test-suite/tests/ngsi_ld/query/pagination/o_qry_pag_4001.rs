//! Following a prev pointer with a different context is bad request data.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType},
};

/// Pins: following the rel="prev" pointer of the second page with an @context other than the one the walk started
/// with is refused with `BadRequestData`, and following it with the same @context answers the first page
/// Clause: 5.5.9.1, 6.3.10
/// Recorded: clause 5.5.9.1 says an attempt to use a different @context during pagination "should result in an
/// `BadRequestData` error"; a "should", so the file is kind O
#[gtest]
#[tokio::test]
async fn o_qry_pag_4001_following_a_prev_pointer_with_a_different_context_is_bad_request_data() -> Result<()> {
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
    let context_2 = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/otherName"}}"#)
            .parse::<InlineContext>()?,
    ));
    let other = HostedContext::add(&broker, &context_2).await?;
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
    let page_2 = linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page.next_page().or_fail()?)
        .await?;
    expect_that!(page_2.prev(), some(anything()));
    let linked_2 = broker.with_context(Context::Single(other.entry()?))?;
    let answer = linked_2
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page_2.prev_page().or_fail()?)
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page_3 = linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page_2.prev_page().or_fail()?)
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    Ok(())
}
