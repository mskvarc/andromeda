//! The language ranges level advertises language tags and language range filtering.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    source_extensions::{extensions, lists_extension},
};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: the broker started at the language-ranges level of `q_target_languages` names both `languageTags` and
/// `languageRangeFiltering` in the extensions list of its Context Source Identity, and the front broker, which
/// keeps the clause 4.9 grammar, names neither
/// Clause: 5.2.40, 6.33
/// Recorded: Andromeda extension, not v1.9.1: Table 5.2.40-1 leaves the content of `contextSourceExtras` to the
/// implementation; Andromeda lists the token of every enabled extension under extensions, and a
/// `q_target_languages` level lists the tokens of the levels below it
#[gtest]
#[tokio::test]
async fn a_ext_adv_0008_the_language_ranges_level_advertises_language_tags_and_language_range_filtering() -> Result<()> {
    let broker_language_range = broker::client(BrokerRole::Edge)?;
    let identity = broker_language_range
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .source_identity()
        .send()
        .await?;
    let listed = extensions(&identity)?;
    expect_that!(&listed, lists_extension("languageTags"));
    expect_that!(&listed, lists_extension("languageRangeFiltering"));
    let broker = broker::front()?;
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .source_identity()
        .send()
        .await?;
    expect_that!(extensions(&received.outcome)?, not(is_empty()));
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("languageTags")));
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("languageRangeFiltering")));
    Ok(())
}
