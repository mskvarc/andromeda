//! A cursor followed under a context expanding a q term to another IRI is refused.

use andromeda_test_suite::{
    broker,
    followed_link::request_without,
    hosted_context::HostedContext,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{HeaderValue, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::{Accept, Link, RelationType},
    },
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType},
    query::{Locality, q},
};

/// Pins: following the next Link of a Query Entities through GET sent under a user @context, whose `q` names a term
/// of it, answers the next page under that @context with or without the `pageContext` the Link carries, and answers
/// 400 `BadRequestData` under another @context that expands the term to another IRI, the `pageContext` left out so
/// only the identity of the query can tell the two @contexts apart
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
        .with_heads()
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(ids.clone())
        .q(q::attr("height").gt(5))
        .locality(Locality::Local)
        .limit(2)
        .send()
        .await?;
    let next = first.outcome.next_page().or_fail()?;
    let same = linked.send(&next).await?;
    expect_that!(same.items(), len(eq(1)));
    let link = first.head.typed::<Link>().valid().cloned().or_fail()?;
    let user_client = broker.with_context(Context::Single(user.entry()?))?;
    let unpinned =
        request_without(&user_client, &link, &RelationType::Next, "pageContext")?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let admitted = user_client.send_unchecked(unpinned).await?;
    expect_that!(admitted.status(), eq(StatusCode::OK));
    expect_that!(entities(&admitted)?, len(eq(1)));
    let other_client = broker.with_context(Context::Single(other.entry()?))?;
    let replayed =
        request_without(&other_client, &link, &RelationType::Next, "pageContext")?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = other_client.send_unchecked(replayed).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
