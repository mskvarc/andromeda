//! List contexts answers every hosted context beyond a thousand.

use andromeda_test_suite::{
    broker,
    broker::BrokerClient,
    context_listing_walk::{ListingWalk, entry_count},
    hosted_context::HostedContext,
    link_header::link_text,
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::model::{Context, ContextEntry, InlineContext};

/// Pins: List @contexts of kind Hosted, with and without details and with no kind filter, walked by its next
/// pointer from a page of 1000, answers every Hosted @context stored, the first and the last of more than a
/// thousand each on exactly one page
/// Clause: 5.13.3.3, 5.13.3.4, 6.29.3.2
/// Tenant: private (the subject is the completeness of the Hosted listing, which only a tenant holding exactly the
/// contexts this file stores can show once more than 1000 exist)
#[gtest]
#[tokio::test]
async fn n_ctx_lst_3001_list_contexts_answers_every_hosted_context_beyond_a_thousand() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let first = add_term(&tenant, &namespace, "first").await?;
    for index in 0..1000 {
        add_term(&tenant, &namespace, &format!("many-{index}")).await?;
    }
    let last = add_term(&tenant, &namespace, "last").await?;
    let (first_id, last_id) = (first.id.to_string(), last.id.to_string());
    // Clause 5.13.3.4 has the broker provide an entry for each @context currently stored, and clause 5.5.9.1 pages
    // the listing, so the walk over its two pages holds the first and the last each on exactly one page.
    for (query, ends_listing) in [
        ("limit=1000&kind=Hosted&details=true", true),
        ("limit=1000&kind=Hosted", true),
        ("limit=1000&details=true", false),
    ] {
        let walk = ListingWalk::read(&tenant, query).await?;
        expect_that!(entry_count(walk.first())?, eq(1000));
        expect_that!(entry_count(walk.second())?, ge(2));
        walk.require_on_one_page(&first_id)?;
        walk.require_on_one_page(&last_id)?;
        if ends_listing {
            expect_that!(link_text(walk.second().headers()), not(contains_substring("rel=\"next\"")));
        }
    }
    Ok(())
}

/// Stores a Hosted @context defining the term `term`, distinct per `term`.
async fn add_term(tenant: &BrokerClient, namespace: &Namespace, term: &str) -> Result<HostedContext> {
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"{term}": "https://test.andromeda/{namespace}/{term}"}}"#).parse::<InlineContext>()?,
    ));
    Ok(HostedContext::add(tenant, &context).await?)
}
