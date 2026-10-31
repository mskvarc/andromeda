//! The front broker advertises every enabled extension.

use andromeda_test_suite::{
    broker,
    source_extensions::{extensions, lists_extension},
};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: a broker started with temporal delete-by-query, purge dataset scope, temporal aggregation grouping,
/// temporal linked retrieval and accept-language enabled names each of them in the extensions list of its Context
/// Source Identity
/// Clause: 5.2.40, 6.33
/// Recorded: Andromeda extension, not v1.9.1: Table 5.2.40-1 leaves the content of `contextSourceExtras` to the
/// implementation; Andromeda lists the token of every enabled extension under extensions
#[gtest]
#[tokio::test]
async fn a_ext_adv_0001_the_front_broker_advertises_every_enabled_extension() -> Result<()> {
    let broker = broker::front()?;
    let identity = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    let listed = extensions(&identity)?;
    expect_that!(listed, not(is_empty()));
    expect_that!(&listed, lists_extension("temporalDeleteByQuery"));
    expect_that!(&listed, lists_extension("purgeDatasetScope"));
    expect_that!(&listed, lists_extension("temporalAggregationGrouping"));
    expect_that!(&listed, lists_extension("temporalLinkedRetrieval"));
    expect_that!(&listed, lists_extension("acceptLanguage"));
    Ok(())
}
