//! A posted temporal query's page link followed under another context is bad request data.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, outcome::Upserted, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Context, ContextEntry, ErrorType, InlineContext, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: following the next Link of Query Temporal Evolution of Entities through POST
/// `/temporal/entityOperations/query` sent under a user @context answers the next page under that @context and with
/// no @context, and answers 400 `BadRequestData` under another @context
/// Clause: 5.5.9.1, 6.3.10, 6.24.3.1
#[gtest]
#[tokio::test]
async fn a_tmp_qry_9098_a_posted_temporal_query_page_link_followed_under_another_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let user = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"Vehicle_{namespace}": "https://test.andromeda/{namespace}/Vehicle"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
    let other = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"Vehicle_{namespace}": "https://test.andromeda/{namespace}/other/Vehicle"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
    let linked = broker
        .with_context(Context::Single(user.entry()?))?
        .with_accept(Accept::new(NgsiLdMediaType::Json));
    for number in 1..=3 {
        let evolution = TemporalEntity::builder()
            .id(namespace.urn("Vehicle", number)?)
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(number).observed_at("2024-01-01T00:00:00Z".parse()?))
            .build()?;
        linked.temporal().create_or_update(&evolution).send().await?.created()?;
    }
    let first = linked
        .temporal()
        .query(temporal_query::after("2023-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
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
