//! List with details describes a hosted context.

use andromeda_test_suite::{broker, context_details, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::{Context, ContextEntry, ContextMetadata, InlineContext, StoredContextKind};

/// Pins: List @contexts with details=true describes an added Hosted entry by its URL, its kind and its `createdAt`
/// Clause: 5.13.3.4, 5.13.3.5, 6.29.3.2
/// Tenant: private (the case asserts what the tenant's whole @context store lists, and clause 4.14 keeps each
/// tenant's stored @contexts its own; in the shared tenant every other file's entries are listed too)
#[gtest]
#[tokio::test]
async fn n_ctx_lst_0009_list_with_details_describes_a_hosted_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let context = Context::Single(ContextEntry::Inline(
        format!(
            r#"{{"A1": "https://test.andromeda/{namespace}/A1", "A2": "https://test.andromeda/{namespace}/A2", "A3": "https://test.andromeda/{namespace}/A3"}}"#
        )
        .parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&tenant, &context).await?;
    // The URL and `createdAt` are members the reader requires, so reading each listed object checks them.
    let listed = context_details::listed(&tenant, None).await?;
    expect_that!(
        listed,
        contains(all!(
            field!(ContextMetadata.local_id, eq(&ctx.id)),
            field!(ContextMetadata.kind, eq(&StoredContextKind::Hosted))
        ))
        .times(eq(1))
    );
    Ok(())
}
