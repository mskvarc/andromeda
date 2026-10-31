//! The details listing holds no core context entry.

use andromeda_test_suite::{broker, config::SuiteConfig, context_details};
use googletest::prelude::*;
use ngsi_ld::model::ContextMetadata;

/// Pins: List @contexts with details lists no core @context entry
/// Clause: 5.13.3.4
/// Recorded: clause 5.13.3.4 lists "@contexts currently stored"; the core @context is not a stored entry, so no
/// listing variant names its URL
#[gtest]
#[tokio::test]
async fn o_ctx_lst_9001_the_details_listing_holds_no_core_context_entry() -> Result<()> {
    let broker = broker::front()?;
    let listed = context_details::listed(&broker, None).await?;
    expect_that!(listed, not(contains(field!(ContextMetadata.url, eq(SuiteConfig::get()?.core_context())))));
    Ok(())
}
