//! A middle page asked by offset points back and on by offset.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    page_link::{carries, carries_parameter, links, naming},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Context Source Registration Subscriptions with limit 1 and offset 1 over three subscriptions answers
/// a prev pointer to offset 0 and a next pointer to offset 2, both on `csourceSubscriptions` with limit 1, and no
/// pointer naming a page parameter
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Tenant: private (Query Context Source Registration Subscriptions pages through every one of the tenant, so in
/// the shared tenant the subscriptions of other files fill the pages; clause 4.14 keeps each tenant's subscriptions
/// its own)
/// Recorded: 6.3.10 says the pointers of the transparent option "should include" limit and offset; Andromeda keeps
/// offset pointers when the request names an offset
#[gtest]
#[tokio::test]
async fn o_css_qry_9002_a_middle_page_asked_by_offset_points_back_and_on_by_offset() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let subscription_0000_id = namespace.subscription_id("0000")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_0001_id = namespace.subscription_id("0001")?;
    let subscription_0002_id = namespace.subscription_id("0002")?;
    // A registration created and deleted again brings the tenant into existence and leaves nothing in it.
    let subscription_0000 = Subscription::registration_builder()
        .id(subscription_0000_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("0000")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.registration_subscriptions().create(&subscription_0000).send().await?;
    let subscription_0001 = Subscription::registration_builder()
        .id(subscription_0001_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("0001")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.registration_subscriptions().create(&subscription_0001).send().await?;
    let subscription_0002 = Subscription::registration_builder()
        .id(subscription_0002_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("0002")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.registration_subscriptions().create(&subscription_0002).send().await?;
    let page = tenant
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .registration_subscriptions()
        .query()
        .limit(1)
        .offset(1)
        .send()
        .await?;
    expect_that!(links(&page), each(not(carries_parameter("page"))));
    expect_that!(page.prev(), some(naming("csourceSubscriptions")));
    expect_that!(page.prev(), some(carries("offset", "0")));
    expect_that!(page.prev(), some(carries("limit", "1")));
    expect_that!(page.next(), some(naming("csourceSubscriptions")));
    expect_that!(page.next(), some(carries("offset", "2")));
    expect_that!(page.next(), some(carries("limit", "1")));
    Ok(())
}
