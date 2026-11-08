//! A middle page points back and forward by offset.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, wire::header_values};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::headers::{Accept, Link},
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Subscriptions with limit 1 and offset 1 over three subscriptions answers prev and next pointers at
/// offsets 0 and 2
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Tenant: private (Query Subscriptions pages through every subscription of the tenant, so in the shared tenant the
/// subscriptions of other files fill the pages; clause 4.14 keeps each tenant's subscriptions its own)
/// Recorded: 6.3.10 says the pointers of the transparent option "should include" limit and offset; Andromeda keeps
/// offset pointers when the request names an offset or an order
#[gtest]
#[tokio::test]
async fn o_sub_qry_0010_a_middle_page_points_back_and_forward_by_offset() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let subscription_0000_id = namespace.subscription_id("0000")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_0001_id = namespace.subscription_id("0001")?;
    let subscription_0002_id = namespace.subscription_id("0002")?;
    let subscription_0000 = Subscription::builder()
        .id(subscription_0000_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0000).send().await?;
    let subscription_0001 = Subscription::builder()
        .id(subscription_0001_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0001).send().await?;
    let subscription_0002 = Subscription::builder()
        .id(subscription_0002_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0002).send().await?;
    let headed = tenant
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .subscriptions()
        .query()
        .limit(1)
        .offset(1)
        .send()
        .await?;
    let page = headed.outcome;
    expect_that!(header_values(headed.head.headers(), &LINK), len(eq(1)));
    expect_that!(
        headed
            .head
            .typed::<Link>()
            .valid()
            .map(|link| link.values().iter().map(|value| value.target().as_str().to_owned()).collect::<Vec<_>>()),
        some(contains(contains_substring("offset=0")))
    );
    expect_that!(
        headed
            .head
            .typed::<Link>()
            .valid()
            .map(|link| link.values().iter().map(|value| value.target().as_str().to_owned()).collect::<Vec<_>>()),
        some(contains(contains_substring("offset=2")))
    );
    expect_that!(page.prev(), some(anything()));
    expect_that!(page.next(), some(anything()));
    Ok(())
}
