//! Each broker answers an identity of its own.

use andromeda_test_suite::{broker, config::BrokerRole};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Retrieve Context Source Identity Information on three brokers answers a `ContextSourceIdentity` each, with
/// an id and a `contextSourceAlias` that differ from the other two
/// Clause: 5.15.1.4, 5.2.40
#[gtest]
#[tokio::test]
async fn n_inf_ret_6001_each_broker_answers_an_identity_of_its_own() -> Result<()> {
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let broker_c = broker::client(BrokerRole::C)?;
    let identity = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    let id_a = identity.id;
    let alias_a = identity.alias;
    let identity_2 = broker_b.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    expect_that!(&identity_2.id, not(eq(&id_a)));
    expect_that!(&identity_2.alias, not(eq(&alias_a)));
    let id_b = identity_2.id;
    let alias_b = identity_2.alias;
    let identity_3 = broker_c.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    expect_that!(&identity_3.id, not(eq(&id_a)));
    expect_that!(&identity_3.id, not(eq(&id_b)));
    expect_that!(&identity_3.alias, not(eq(&alias_a)));
    expect_that!(&identity_3.alias, not(eq(&alias_b)));
    Ok(())
}
