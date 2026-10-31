//! List of more than a hundred contexts is paged and the pages hold every one.

use andromeda_test_suite::{
    broker,
    context_details,
    context_listing_walk::{ListingWalk, entry_count},
    hosted_context::HostedContext,
    link_header::link_text,
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::model::{Context, ContextEntry, ContextMetadata, InlineContext, StoredContextKind};

/// Pins: List @contexts of kind Hosted with details, over 120 stored Hosted @contexts and no limit, answers a first
/// page of 100 with a next pointer, and the page it points to holds the other 20 and no next pointer
/// Clause: 4.12, 5.13.3.4, 6.29.3.2, 6.3.10
/// Tenant: private (the subject is the exact size of the Hosted listing, which only a tenant holding exactly the
/// contexts this file stores can show)
/// Recorded: clause 5.13.3.4 has the broker provide an entry for each stored @context and Table 6.29.3.2-1 defines
/// no paging parameter; clause 4.12 mandates paging for five query operations and forbids it nowhere, so Andromeda
/// pages List @contexts as it pages them, 100 entries by default, linking the next page as clause 6.3.10 does
#[gtest]
#[tokio::test]
async fn o_ctx_lst_0018_list_of_more_than_a_hundred_contexts_is_paged_and_the_pages_hold_every_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    for index in 0..120 {
        let context = Context::Single(ContextEntry::Inline(
            format!(r#"{{"term": "https://test.andromeda/{namespace}/many-{index}"}}"#).parse::<InlineContext>()?,
        ));
        HostedContext::add(&tenant, &context).await?;
    }
    let walk = ListingWalk::read(&tenant, "kind=Hosted&details=true").await?;
    let page_one = context_details::metadata_list(walk.first())?;
    expect_that!(page_one, len(eq(100)));
    expect_that!(page_one, each(field!(ContextMetadata.kind, eq(&StoredContextKind::Hosted))));
    expect_that!(link_text(walk.first().headers()), contains_substring("rel=\"next\""));
    let first_on_page_one = page_one.first().or_fail()?.local_id.clone();
    let last_on_page_one = page_one.last().or_fail()?.local_id.clone();
    let page_two = context_details::metadata_list(walk.second())?;
    expect_that!(entry_count(walk.second())?, eq(20));
    expect_that!(page_two, each(field!(ContextMetadata.kind, eq(&StoredContextKind::Hosted))));
    expect_that!(page_two, not(contains(field!(ContextMetadata.local_id, eq(&first_on_page_one)))));
    expect_that!(page_two, not(contains(field!(ContextMetadata.local_id, eq(&last_on_page_one)))));
    expect_that!(link_text(walk.second().headers()), not(contains_substring("rel=\"next\"")));
    Ok(())
}
