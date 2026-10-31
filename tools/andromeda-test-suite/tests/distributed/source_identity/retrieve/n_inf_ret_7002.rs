//! Each broker answers an alias of its own.

use andromeda_test_suite::{broker, config::BrokerRole};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Retrieve Context Source Identity Information on broker-a, broker-b and broker-c answers three non-empty
/// `contextSourceAlias` values that differ pairwise
/// Clause: 5.15.1.4, 5.2.40
#[gtest]
#[tokio::test]
async fn n_inf_ret_7002_each_broker_answers_an_alias_of_its_own() -> Result<()> {
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let broker_c = broker::client(BrokerRole::C)?;
    let identity = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    let alias_a = identity.alias;
    let identity_2 = broker_b.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    expect_that!(&identity_2.alias, not(eq(&alias_a)));
    let alias_b = identity_2.alias;
    let identity_3 = broker_c.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    expect_that!(&identity_3.alias, not(eq(&alias_a)));
    expect_that!(&identity_3.alias, not(eq(&alias_b)));
    Ok(())
}
