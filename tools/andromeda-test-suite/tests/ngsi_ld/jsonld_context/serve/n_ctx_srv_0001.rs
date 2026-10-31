//! Serve with details answers the metadata of a hosted context.

use andromeda_test_suite::{broker, context_details, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::{Context, ContextEntry, InlineContext, StoredContextKind};

/// Pins: Serve @context with details=true answers 200 with the metadata of a Hosted entry: its kind, its local
/// identifier, a URL and a `createdAt`
/// Clause: 5.13.4.4, 5.13.3.5, 6.30.3.1
#[gtest]
#[tokio::test]
async fn n_ctx_srv_0001_serve_with_details_answers_the_metadata_of_a_hosted_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(
            r#"{{"A1": "https://test.andromeda/{namespace}/A1", "A2": "https://test.andromeda/{namespace}/A2", "A3": "https://test.andromeda/{namespace}/A3"}}"#
        )
        .parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    // The URL and `createdAt` are members the reader requires, so reading the answer checks them.
    let answer = context_details::serve_details(&broker, &ctx.id).await?;
    let metadata = context_details::metadata(&answer)?;
    expect_that!(metadata.kind, eq(&StoredContextKind::Hosted));
    expect_that!(metadata.local_id, eq(&ctx.id));
    Ok(())
}
