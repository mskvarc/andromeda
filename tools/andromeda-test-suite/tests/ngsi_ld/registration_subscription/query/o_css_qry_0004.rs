//! The first page of a cursor walk offers opaque cursor pointers.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    cursor_walk,
    namespace::Namespace,
    page_link::{carries_parameter, links},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QuerySubscriptions},
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, RegistrationSubscription, Subscription},
};

/// Pins: Query Context Source Registration Subscriptions with limit 2 and an empty cursor over three members answers
/// a first page whose Link offers next and last pointers carrying a cursor and no offset, and no first or prev
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Tenant: private (Query Context Source Registration Subscriptions pages through every one of the tenant, so in
/// the shared tenant the subscriptions of other files fill the pages; clause 4.14 keeps each tenant's subscriptions
/// its own)
/// Recorded: 5.5.9.2 lets the pointers of 5.5.9.1 be "implemented in a completely opaque way"; Andromeda walks a
/// query in its default order by an opaque cursor once the request sends an empty `cursor`, and offers first and last
/// pointers beside next and prev
#[gtest]
#[tokio::test]
async fn o_css_qry_0004_the_first_page_of_a_cursor_walk_offers_opaque_cursor_pointers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let subscription_0000_id = namespace.subscription_id("0000")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_0001_id = namespace.subscription_id("0001")?;
    let subscription_0002_id = namespace.subscription_id("0002")?;
    let subscription_0000 = Subscription::registration_builder()
        .id(subscription_0000_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.registration_subscriptions().create(&subscription_0000).send().await?;
    let subscription_0001 = Subscription::registration_builder()
        .id(subscription_0001_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.registration_subscriptions().create(&subscription_0001).send().await?;
    let subscription_0002 = Subscription::registration_builder()
        .id(subscription_0002_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.registration_subscriptions().create(&subscription_0002).send().await?;
    let query = QuerySubscriptions::builder().kind(RegistrationSubscription).limit(2).build();
    let page = cursor_walk::first_page(&tenant.with_accept(Accept::new(NgsiLdMediaType::JsonLd)), &query)
        .await?
        .page;
    expect_that!(page.next(), some(anything()));
    expect_that!(page.last(), some(anything()));
    expect_that!(page.first(), none());
    expect_that!(page.prev(), none());
    expect_that!(links(&page), contains(carries_parameter("cursor")));
    expect_that!(links(&page), each(not(carries_parameter("offset"))));
    Ok(())
}
