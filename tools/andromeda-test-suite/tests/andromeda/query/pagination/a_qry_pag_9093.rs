//! A posted query's page link followed under another context is bad request data.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::QueryMethod},
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType},
};

/// Pins: following the next Link of Query Entities through POST `/entityOperations/query` sent under a user @context
/// answers the next page under that @context and with no @context, and answers 400 `BadRequestData` under another
/// @context
/// Clause: 5.5.9.1, 6.3.10, 6.23.3.1
#[gtest]
#[tokio::test]
async fn a_qry_pag_9093_a_posted_query_page_link_followed_under_another_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let user = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"Building_{namespace}": "https://test.andromeda/{namespace}/Building"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
    let other = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"Building_{namespace}": "https://test.andromeda/{namespace}/other/Building"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
    let body_context = broker
        .with_context(Context::Single(user.entry()?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let ids = vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?, namespace.urn("Building", 3)?];
    for id in &ids {
        let building = Entity::builder().id(id.clone()).entity_type(building_type.clone()).build()?;
        body_context.entities().create(&building).send().await?.done()?;
    }
    let linked = broker
        .with_context(Context::Single(user.entry()?))?
        .with_accept(Accept::new(NgsiLdMediaType::Json));
    let first = linked
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&building_type)
        .ids(ids.clone())
        .limit(2)
        .send()
        .await?;
    let next = first.next_page().or_fail()?;
    let same = linked.send(&next).await?;
    expect_that!(same.items(), len(eq(1)));
    let unnamed = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).send(&next).await?;
    expect_that!(unnamed.items(), len(eq(1)));
    let answer = broker
        .with_context(Context::Single(other.entry()?))?
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .send(&next)
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
