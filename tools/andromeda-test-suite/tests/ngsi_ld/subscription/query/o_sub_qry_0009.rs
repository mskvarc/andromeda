//! An offset listing is in identifier order.

use andromeda_test_suite::{broker, config::SuiteConfig, identity_matcher::ids_in_order, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Subscriptions with offset 0 and limit 5 over five subscriptions created in descending identifier
/// order answers them in ascending identifier order
/// Clause: 5.5.9.2, 6.3.10
/// Tenant: private (Query Subscriptions pages through every subscription of the tenant, so in the shared tenant the
/// subscriptions of other files fill the pages; clause 4.14 keeps each tenant's subscriptions its own)
/// Recorded: no order is fixed for a query without `orderBy`; Andromeda pages in ascending identifier order
#[gtest]
#[tokio::test]
async fn o_sub_qry_0009_an_offset_listing_is_in_identifier_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let subscription_0004_id = namespace.subscription_id("0004")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_0003_id = namespace.subscription_id("0003")?;
    let subscription_0002_id = namespace.subscription_id("0002")?;
    let subscription_0001_id = namespace.subscription_id("0001")?;
    let subscription_0000_id = namespace.subscription_id("0000")?;
    let subscription_0004 = Subscription::builder()
        .id(subscription_0004_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0004).send().await?;
    let subscription_0003 = Subscription::builder()
        .id(subscription_0003_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0003).send().await?;
    let subscription_0002 = Subscription::builder()
        .id(subscription_0002_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0002).send().await?;
    let subscription_0001 = Subscription::builder()
        .id(subscription_0001_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0001).send().await?;
    let subscription_0000 = Subscription::builder()
        .id(subscription_0000_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0000).send().await?;
    let page = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .query()
        .offset(0)
        .limit(5)
        .send()
        .await?;
    let expected_ids = (0..=4).map(|n| namespace.subscription_id(format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    expect_that!(page.items(), ids_in_order(&expected_ids));
    Ok(())
}
