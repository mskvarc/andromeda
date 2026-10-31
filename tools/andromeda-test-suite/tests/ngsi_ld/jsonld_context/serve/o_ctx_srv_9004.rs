//! A use is counted by the time the request has answered.

use andromeda_test_suite::{broker, context_details, context_referencing_create, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::{Context, ContextEntry, InlineContext};

/// Pins: A Hosted @context reports `numberOfHits` 0 when stored and still 0 after its metadata is read twice, and 1
/// once a Create Entity naming its URL has answered
/// Clause: 5.13.3.5, 5.13.4.4
/// Recorded: `numberOfHits` is optional (clause 5.13.3.5) and the clause does not say what one find is; Andromeda
/// counts one per NGSI-LD operation that resolves the @context, visible as soon as that operation has answered
#[gtest]
#[tokio::test]
async fn o_ctx_srv_9004_a_use_is_counted_by_the_time_the_request_has_answered() -> Result<()> {
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
    let read_again = context_details::served(&broker, &ctx.id).await?;
    expect_that!(read_again.number_of_hits, some(eq(0)));
    // The document gives `A1` a bare value, which the entity model never writes, so it is sent as text.
    context_referencing_create::create(
        &broker,
        &namespace.urn("Testing", "absolute")?,
        &namespace.entity_type("Testing")?,
        stored.url.as_str(),
    )
    .await?;
    let used = context_details::served(&broker, &ctx.id).await?;
    expect_that!(used.number_of_hits, some(eq(1)));
    Ok(())
}
