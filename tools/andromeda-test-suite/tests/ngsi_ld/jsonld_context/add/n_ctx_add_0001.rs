//! Add a term dictionary stores a hosted context.

use andromeda_test_suite::{broker, context_details, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::{Context, ContextEntry, InlineContext, StoredContextKind};

/// Pins: Add @context with a dictionary of terms answers 201 with a Location, and the entry it names is of kind
/// Hosted
/// Clause: 5.13.2.4, 5.13.4.4, 6.29.3.1
#[gtest]
#[tokio::test]
async fn n_ctx_add_0001_add_a_term_dictionary_stores_a_hosted_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(
            r#"{{"A1": "https://test.andromeda/{namespace}/A1", "A2": "https://test.andromeda/{namespace}/A2", "A3": "https://test.andromeda/{namespace}/A3"}}"#
        )
        .parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    let answer = context_details::serve_details(&broker, &ctx.id).await?;
    expect_that!(context_details::metadata(&answer)?.kind, eq(&StoredContextKind::Hosted));
    Ok(())
}
