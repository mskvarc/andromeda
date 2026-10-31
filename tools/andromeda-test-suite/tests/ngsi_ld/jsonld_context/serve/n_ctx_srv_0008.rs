//! Serve answers the document added and lists its entry.

use andromeda_test_suite::{broker, context_details, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ContextKindFilter},
    model::{Context, ContextEntry, InlineContext, StoredContextKind},
};

/// Pins: Serve @context answers exactly the document Add @context stored, its details name the entry by kind Hosted
/// and its local identifier, and List @contexts with details lists that entry as Hosted
/// Clause: 5.13.2.4, 5.13.4.4, 5.13.3.4, 5.13.3.5
/// Tenant: private (the case asserts what the tenant's whole @context store lists, and clause 4.14 keeps each
/// tenant's stored @contexts its own; in the shared tenant every other file's entries are listed too)
#[gtest]
#[tokio::test]
async fn n_ctx_srv_0008_serve_answers_the_document_added_and_lists_its_entry() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let context = Context::Single(ContextEntry::Inline(
        format!(
            r#"{{"A1": "https://test.andromeda/{namespace}/A1", "A2": {{"@id": "https://test.andromeda/{namespace}/A2", "@type": "@vocab"}}, "A3": "https://test.andromeda/{namespace}/A3"}}"#
        )
        .parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&tenant, &context).await?;
    let served = tenant.with_accept(Accept::any()).contexts().serve(&ctx.id).send().await?;
    expect_that!(served, eq(&context));
    let answer = context_details::serve_details(&tenant, &ctx.id).await?;
    let metadata = context_details::metadata(&answer)?;
    expect_that!(metadata.kind, eq(&StoredContextKind::Hosted));
    expect_that!(metadata.local_id, eq(&ctx.id));
    let listing = context_details::list_details(&tenant, Some(ContextKindFilter::Hosted)).await?;
    let listed = context_details::metadata_list(&listing)?;
    expect_that!(
        listed.iter().find(|entry| entry.local_id == ctx.id).map(|entry| &entry.kind),
        some(eq(&StoredContextKind::Hosted))
    );
    Ok(())
}
