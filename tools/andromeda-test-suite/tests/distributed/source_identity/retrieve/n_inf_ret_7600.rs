//! Each broker answers its own conformant identity.

use andromeda_test_suite::{broker, config::BrokerRole};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Retrieve Context Source Identity Information on broker A and broker B with Accept application/ld+json
/// answers 200 with a `ContextSourceIdentity` holding a URI id, a non-empty alias, a duration uptime and a
/// `DateTime` `timeAt`; B differs from A
/// Clause: 5.15.1.4, 5.2.40
#[gtest]
#[tokio::test]
async fn n_inf_ret_7600_each_broker_answers_its_own_conformant_identity() -> Result<()> {
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let identity = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).source_identity().send().await?;
    let a_id = identity.id;
    let a_alias = identity.alias;
    let identity_2 = broker_b.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).source_identity().send().await?;
    expect_that!(&identity_2.id, not(eq(&a_id)));
    expect_that!(&identity_2.alias, not(eq(&a_alias)));
    Ok(())
}
