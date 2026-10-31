//! Each request counts a single find.

use andromeda_test_suite::{broker, context_details, context_referencing_create, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::{Context, ContextEntry, InlineContext};

/// Pins: Each Create Entity naming the root-relative path of a Hosted @context adds exactly one to its
/// `numberOfHits`, however often the broker resolves the @context while serving the request
/// Clause: 5.13.3.5
/// Recorded: `numberOfHits` is optional (clause 5.13.3.5) and the clause does not say what one find is; Andromeda
/// counts one per NGSI-LD operation that resolves the @context, visible as soon as that operation has answered
#[gtest]
#[tokio::test]
async fn o_ctx_srv_9006_each_request_counts_a_single_find() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let ctx = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"A1": "urn:ngsi-ld:attributes:{namespace}-A1"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
    let testing_type = namespace.entity_type("Testing")?;
    for (suffix, hits) in [("one", 1), ("two", 2)] {
        // The document is sent as text: the client refuses a relative reference in an `@context` array.
        context_referencing_create::create(&broker, &namespace.urn("Testing", suffix)?, &testing_type, &ctx.location).await?;
        let used = context_details::served(&broker, &ctx.id).await?;
        expect_that!(used.number_of_hits, some(eq(hits)));
    }
    Ok(())
}
