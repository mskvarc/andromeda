//! A relative reference is resolved and counted once.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_details,
    context_referencing_create,
    entity_matcher::has_attribute,
    hosted_context::HostedContext,
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType},
};

/// Pins: Create Entity naming as its @context the root-relative path Add @context answered with expands the entity
/// through the stored @context, and `numberOfHits` goes from 0 to exactly 1
/// Clause: 5.13.2.5, 5.13.3.5
/// Recorded: `numberOfHits` is optional (clause 5.13.3.5) and the clause does not say what one find is; Andromeda
/// counts one per NGSI-LD operation that resolves the @context, visible as soon as that operation has answered; the
/// specification does not say how a relative @context reference resolves, and Andromeda resolves its own Location
#[gtest]
#[tokio::test]
async fn o_ctx_srv_9005_a_relative_reference_is_resolved_and_counted_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let ctx = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"A1": "urn:ngsi-ld:attributes:{namespace}-A1"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
    let stored = context_details::served(&broker, &ctx.id).await?;
    expect_that!(stored.number_of_hits, some(eq(0)));
    let relative_id = namespace.urn("Testing", "relative")?;
    // The document is sent as text: the client refuses a relative reference in an `@context` array.
    context_referencing_create::create(&broker, &relative_id, &namespace.entity_type("Testing")?, &ctx.location).await?;
    let used = context_details::served(&broker, &ctx.id).await?;
    expect_that!(used.number_of_hits, some(eq(1)));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&relative_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute(&format!("urn:ngsi-ld:attributes:{namespace}-A1"), any_attribute().value("a value"))
    );
    Ok(())
}
