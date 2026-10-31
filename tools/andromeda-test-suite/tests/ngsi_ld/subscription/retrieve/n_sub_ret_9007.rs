//! An endpoint cooldown and timeout above 32 bits read back as written.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription whose notification endpoint carries a cooldown and a timeout of 4294967296000 milliseconds, past the 32-bit range, is created and reads both back unchanged
/// Clause: Table 5.2.15-1, 5.8.1, 5.8.3
#[gtest]
#[tokio::test]
async fn n_sub_ret_9007_an_endpoint_cooldown_and_timeout_above_32_bits_read_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_wide_id = namespace.subscription_id("wide")?;
    let building_wide_id = namespace.urn("Building", "wide")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_wide = Subscription::builder()
        .id(subscription_wide_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_wide_id)])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(format!("{unreachable}/notify"))
                    .accept(NgsiLdMediaType::Json)
                    .cooldown(4_294_967_296_000_i64)
                    .timeout(4_294_967_296_000_i64)
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_wide).send().await?;
    let subscription = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_wide_id)
        .send()
        .await?;
    expect_that!(
        subscription.notification().endpoint().cooldown().and_then(|number| number.as_ref().as_i64()),
        some(eq(4_294_967_296_000))
    );
    expect_that!(
        subscription.notification().endpoint().timeout().and_then(|number| number.as_ref().as_i64()),
        some(eq(4_294_967_296_000))
    );
    broker.subscriptions().delete(&subscription_wide_id).send().await?;
    Ok(())
}
