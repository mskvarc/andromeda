//! A middle page links the previous and the next page.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    page_link::{carries, naming},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Subscriptions with limit=1 and offset=1, with at least four Subscriptions held, answers a Link
/// header whose prev link names offset 0 and whose next link names offset 2, both on the subscriptions endpoint
/// with limit 1
/// Clause: 5.5.9, 6.3.10, 6.10.3.2
#[gtest]
#[tokio::test]
async fn n_sub_qry_9001_a_middle_page_links_the_previous_and_the_next_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_pagination_a_id = namespace.subscription_id("pagination-a")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_pagination_b_id = namespace.subscription_id("pagination-b")?;
    let subscription_pagination_c_id = namespace.subscription_id("pagination-c")?;
    let subscription_pagination_d_id = namespace.subscription_id("pagination-d")?;
    let subscription_pagination_a = Subscription::builder()
        .id(subscription_pagination_a_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_pagination_a).send().await?;
    let subscription_pagination_b = Subscription::builder()
        .id(subscription_pagination_b_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_pagination_b).send().await?;
    let subscription_pagination_c = Subscription::builder()
        .id(subscription_pagination_c_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_pagination_c).send().await?;
    let subscription_pagination_d = Subscription::builder()
        .id(subscription_pagination_d_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_pagination_d).send().await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .query()
        .limit(1)
        .offset(1)
        .send()
        .await?;
    expect_that!(page.prev(), some(naming("/subscriptions")));
    expect_that!(page.prev(), some(carries("offset", "0")));
    expect_that!(page.prev(), some(carries("limit", "1")));
    expect_that!(page.next(), some(naming("/subscriptions")));
    expect_that!(page.next(), some(carries("offset", "2")));
    expect_that!(page.next(), some(carries("limit", "1")));
    broker.subscriptions().delete(&subscription_pagination_a_id).send().await?;
    broker.subscriptions().delete(&subscription_pagination_b_id).send().await?;
    broker.subscriptions().delete(&subscription_pagination_c_id).send().await?;
    broker.subscriptions().delete(&subscription_pagination_d_id).send().await?;
    Ok(())
}
