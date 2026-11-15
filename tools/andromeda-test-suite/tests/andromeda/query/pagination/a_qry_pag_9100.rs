//! A cursor followed under a context expanding a q term to another IRI is refused.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType},
    query::{Locality, q},
};

/// Pins: following the next Link of a Query Entities through GET sent under a user @context, whose `q` names a term
/// of it, answers the next page under that @context, and answers 400 `BadRequestData` under another @context that
/// expands the term to another IRI
/// Clause: 5.5.9.1, 6.3.10
#[gtest]
#[tokio::test]
async fn a_qry_pag_9100_a_cursor_followed_under_a_context_expanding_a_q_term_to_another_iri_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let user = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "height": "https://test.andromeda/{namespace}/height"}}"#)
                .parse::<InlineContext>()?,
        )),
    )
    .await?;
    let other = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(
                r#"{{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "height": "https://test.andromeda/{namespace}/other/height"}}"#
            )
            .parse::<InlineContext>()?,
        )),
    )
    .await?;
    let body_context = broker
        .with_context(Context::Single(user.entry()?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let ids = vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?, namespace.urn("Building", 3)?];
    for id in &ids {
        let building = Entity::builder()
            .id(id.clone())
            .entity_type(building_type.clone())
            .property("height", 10)
            .build()?;
        body_context.entities().create(&building).send().await?.done()?;
    }
    let linked = broker
        .with_context(Context::Single(user.entry()?))?
        .with_accept(Accept::new(NgsiLdMediaType::Json));
    let first = linked
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(ids.clone())
        .q(q::attr("height").gt(5))
        .locality(Locality::Local)
        .limit(2)
        .send()
        .await?;
    let next = first.next_page().or_fail()?;
    let same = linked.send(&next).await?;
    expect_that!(same.items(), len(eq(1)));
    let answer = broker
        .with_context(Context::Single(other.entry()?))?
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .send(&next)
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
