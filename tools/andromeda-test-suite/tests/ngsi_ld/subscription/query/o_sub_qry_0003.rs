//! Following next returns every member once in identifier order.

use andromeda_test_suite::{broker, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, nth::nth};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Subscriptions with limit 2 over 7 members, followed by its next pointers, answers every member once
/// in ascending identifier order, the last page with a prev pointer and no next
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Tenant: private (Query Subscriptions pages through every subscription of the tenant, so in the shared tenant the
/// subscriptions of other files fill the pages; clause 4.14 keeps each tenant's subscriptions its own)
/// Recorded: no order is fixed for a query without `orderBy`; Andromeda pages in ascending identifier order
#[gtest]
#[tokio::test]
async fn o_sub_qry_0003_following_next_returns_every_member_once_in_identifier_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let subscription_0000_id = namespace.subscription_id("0000")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_0001_id = namespace.subscription_id("0001")?;
    let subscription_0002_id = namespace.subscription_id("0002")?;
    let subscription_0003_id = namespace.subscription_id("0003")?;
    let subscription_0004_id = namespace.subscription_id("0004")?;
    let subscription_0005_id = namespace.subscription_id("0005")?;
    let subscription_0006_id = namespace.subscription_id("0006")?;
    let subscription_0000 = Subscription::builder()
        .id(subscription_0000_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0000).send().await?;
    let subscription_0001 = Subscription::builder()
        .id(subscription_0001_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0001).send().await?;
    let subscription_0002 = Subscription::builder()
        .id(subscription_0002_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0002).send().await?;
    let subscription_0003 = Subscription::builder()
        .id(subscription_0003_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0003).send().await?;
    let subscription_0004 = Subscription::builder()
        .id(subscription_0004_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0004).send().await?;
    let subscription_0005 = Subscription::builder()
        .id(subscription_0005_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0005).send().await?;
    let subscription_0006 = Subscription::builder()
        .id(subscription_0006_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0006).send().await?;
    let page = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .query()
        .limit(2)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&subscription_0000_id)));
    expect_that!(page.items(), nth(1, has_id(&subscription_0001_id)));
    let page_2 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .send(&page.next_page().or_fail()?)
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&subscription_0002_id)));
    expect_that!(page_2.items(), nth(1, has_id(&subscription_0003_id)));
    let page_3 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .send(&page_2.next_page().or_fail()?)
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), nth(0, has_id(&subscription_0004_id)));
    expect_that!(page_3.items(), nth(1, has_id(&subscription_0005_id)));
    let page_4 = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .send(&page_3.next_page().or_fail()?)
        .await?;
    expect_that!(page_4.next(), none());
    expect_that!(page_4.prev(), some(anything()));
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&subscription_0006_id)));
    Ok(())
}
