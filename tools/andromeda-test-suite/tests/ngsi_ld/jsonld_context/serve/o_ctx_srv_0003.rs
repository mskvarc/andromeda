//! Serve counts each use of a hosted context.

use andromeda_test_suite::{broker, context_details, hosted_context::HostedContext, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Context, ContextEntry, Entity, InlineContext},
};

/// Pins: A create naming a Hosted @context raises its `numberOfHits` by the time the create has answered
/// Clause: 5.13.3.5, 5.13.4.4
/// Recorded: `numberOfHits` is optional (clause 5.13.3.5); Andromeda reports it, and counts a use before the
/// request that made it returns
#[gtest]
#[tokio::test]
async fn o_ctx_srv_0003_serve_counts_each_use_of_a_hosted_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(
            r#"{{"A1": "https://test.andromeda/{namespace}/A1", "A2": "https://test.andromeda/{namespace}/A2", "A3": "https://test.andromeda/{namespace}/A3"}}"#
        )
        .parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    let before = context_details::metadata(&context_details::serve_details(&broker, &ctx.id).await?)?;
    let hits = before.number_of_hits.or_fail()?;
    let named = HostedContext {
        id: ctx.id.clone(),
        location: before.url.path().to_owned(),
        url: before.url,
    };
    let testing = Entity::builder()
        .id(namespace.urn("Testing", 1)?)
        .entity_type(namespace.entity_type("Testing")?)
        .property("A1", "value of A1")
        .build()?;
    broker
        .with_context(Context::Single(named.entry()?))?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .entities()
        .create(&testing)
        .send()
        .await?
        .done()?;
    let after = context_details::metadata(&context_details::serve_details(&broker, &ctx.id).await?)?;
    expect_that!(after.number_of_hits, some(gt(hits)));
    Ok(())
}
