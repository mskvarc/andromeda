//! The language tags level advertises language tags alone.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    source_extensions::{extensions, lists_extension},
};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: the broker started at the language-tags level of `q_target_languages` names `languageTags` in the
/// extensions list of its Context Source Identity and does not name `languageRangeFiltering`
/// Clause: 5.2.40, 6.33
/// Recorded: Andromeda extension, not v1.9.1: Table 5.2.40-1 leaves the content of `contextSourceExtras` to the
/// implementation; Andromeda lists the token of every enabled extension under extensions
#[gtest]
#[tokio::test]
async fn a_ext_adv_0009_the_language_tags_level_advertises_language_tags_alone() -> Result<()> {
    let broker_language_tags = broker::client(BrokerRole::Prefixed)?;
    let received = broker_language_tags
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .source_identity()
        .send()
        .await?;
    expect_that!(&extensions(&received.outcome)?, lists_extension("languageTags"));
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("languageRangeFiltering")));
    Ok(())
}
