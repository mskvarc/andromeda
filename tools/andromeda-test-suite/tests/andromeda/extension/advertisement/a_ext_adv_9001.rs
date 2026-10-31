//! Temporal linked retrieval is not advertised by default.

use andromeda_test_suite::{broker, config::BrokerRole};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Retrieve Context Source Identity of a broker running the default configuration does not advertise the
/// `temporalLinkedRetrieval` extension
/// Clause: -
/// Recorded: Andromeda extension, not v1.9.1: Linked Entity retrieval on the temporal operations, off by default
#[gtest]
#[tokio::test]
async fn a_ext_adv_9001_temporal_linked_retrieval_is_not_advertised_by_default() -> Result<()> {
    let broker_not_recorded = broker::client(BrokerRole::NotRecorded)?;
    // This broker leaves every extension flag at its default.
    let received = broker_not_recorded
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .source_identity()
        .send()
        .await?;
    expect_that!(String::from_utf8_lossy(&received.body), not(contains_substring("temporalLinkedRetrieval")));
    Ok(())
}
