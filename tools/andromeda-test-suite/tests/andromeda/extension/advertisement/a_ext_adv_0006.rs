//! Conformance mode advertises no extension.

use andromeda_test_suite::{broker, config::BrokerRole};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{ContextSourceAlias, NgsiLdMediaType},
};

/// Pins: a broker in conformance mode, with the extension flags also on, names none of those extensions, nor the
/// default-on cursor pagination, in its Context Source Identity
/// Clause: 5.2.40, 6.33
/// Recorded: Andromeda extension, not v1.9.1: Table 5.2.40-1 leaves the content of `contextSourceExtras` to the
/// implementation; conformance mode forces every extension off whatever its own flag says, cursor pagination
/// included
#[gtest]
#[tokio::test]
async fn a_ext_adv_0006_conformance_mode_advertises_no_extension() -> Result<()> {
    let broker_conformance = broker::client(BrokerRole::Conformance)?;
    let received = broker_conformance
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .source_identity()
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("temporalDeleteByQuery")));
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("purgeDatasetScope")));
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("temporalAggregationGrouping")));
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("temporalLinkedRetrieval")));
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("cursorPagination")));
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("acceptLanguage")));
    let identity = &received.outcome;
    expect_that!(&identity.alias, eq(&"broker-conformance".parse::<ContextSourceAlias>()?));
    Ok(())
}
