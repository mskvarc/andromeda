//! An endpoint cooldown and timeout read back as written.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription whose notification endpoint carries cooldown 2500 and timeout 750 reads both back
/// Clause: Table 5.2.15-1, 5.8.3
#[gtest]
#[tokio::test]
async fn n_sub_ret_9004_an_endpoint_cooldown_and_timeout_read_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_endpoint_id = namespace.subscription_id("endpoint")?;
    let building_endpoint_id = namespace.urn("Building", "endpoint")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_endpoint = Subscription::builder()
        .id(subscription_endpoint_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_endpoint_id)])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(format!("{unreachable}/notify"))
                    .accept(NgsiLdMediaType::Json)
                    .timeout(750)
                    .cooldown(2500)
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_endpoint).send().await?;
    let subscription = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_endpoint_id)
        .send()
        .await?;
    expect_that!(
        subscription.notification().endpoint().cooldown().and_then(|number| number.as_ref().as_i64()),
        some(eq(2500))
    );
    expect_that!(
        subscription.notification().endpoint().timeout().and_then(|number| number.as_ref().as_i64()),
        some(eq(750))
    );
    broker.subscriptions().delete(&subscription_endpoint_id).send().await?;
    Ok(())
}
