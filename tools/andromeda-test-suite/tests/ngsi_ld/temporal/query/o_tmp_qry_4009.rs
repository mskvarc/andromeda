//! Following a next pointer with a different context is bad request data.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, outcome::Upserted, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, ErrorType, InlineContext, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: following the rel="next" pointer of a Query Temporal Evolution of Entities page with an @context other
/// than the one the walk started with is refused with `BadRequestData`
/// Clause: 5.5.9.1, 6.3.10, 5.7.4
/// Recorded: clause 5.5.9.1 says an attempt to use a different @context during pagination "should result in an
/// `BadRequestData` error"; a "should", so the file is kind O
#[gtest]
#[tokio::test]
async fn o_tmp_qry_4009_following_a_next_pointer_with_a_different_context_is_bad_request_data() -> Result<()> {
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
    let building_evolution = TemporalEntity::builder()
        .id(building_id)
        .entity_type(building_type.clone())
        .property("name", Property::builder("b1").observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    body_context.temporal().create_or_update(&building_evolution).send().await?.created()?;
    let building_2_evolution = TemporalEntity::builder()
        .id(building_2_id)
        .entity_type(building_type.clone())
        .property("name", Property::builder("b2").observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    body_context.temporal().create_or_update(&building_2_evolution).send().await?.created()?;
    let building_3_evolution = TemporalEntity::builder()
        .id(building_3_id)
        .entity_type(building_type.clone())
        .property("name", Property::builder("b3").observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    body_context.temporal().create_or_update(&building_3_evolution).send().await?.created()?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::after("2020-07-01T00:00:00Z".parse()?))
        .entity_type(&building_type)
        .limit(1)
        .send()
        .await?;
    expect_that!(page.next(), some(anything()));
    let linked_2 = broker.with_context(Context::Single(other.entry()?))?;
    let answer = linked_2
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page.next_page().or_fail()?)
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
