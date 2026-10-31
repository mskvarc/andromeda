//! A periodic subscription matching nothing reports no last notification.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a Subscription with `timeInterval` 1 on an Entity that does not exist is sent nothing and keeps reporting
/// no `timesSent` and no `lastNotification` after its interval has been reached several times, while one on an
/// existing Entity is notified
/// Clause: 5.8.6, 5.2.14.2
///
/// Clause 5.8.6: "If there are no matching Entities, no Notification is sent", and Table 5.2.14.2-1 makes
/// `lastNotification` the instant the last notification has been sent. The silent Subscription is created first, so
/// the third notification of the sentinel comes after at least two of its intervals.
#[gtest]
#[tokio::test]
async fn n_ntf_sta_0004_a_periodic_subscription_matching_nothing_reports_no_last_notification() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_silent_id = namespace.subscription_id("silent")?;
    let vehicle_absent_id = namespace.urn("Vehicle", "absent")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_present_id = namespace.urn("Vehicle", "present")?;
    let subscription_sentinel_id = namespace.subscription_id("sentinel")?;
    let subscription_silent = Subscription::builder()
        .id(subscription_silent_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_absent_id)])
        .time_interval(1)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("silent")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_silent).send().await?;
    let vehicle_present = Entity::builder()
        .id(vehicle_present_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle_present).send().await?.done()?;
    let subscription_sentinel = Subscription::builder()
        .id(subscription_sentinel_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_present_id.clone())])
        .time_interval(1)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("sentinel")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_sentinel).send().await?;
    sink.wait_for_at_least("sentinel", 3).await?;
    let notifications = sink.received("silent").await?;
    expect_that!(notifications, len(eq(0)));
    let subscription = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_silent_id)
        .send()
        .await?;
    expect_that!(subscription.notification().output().times_sent, none());
    expect_that!(subscription.notification().output().last_notification, none());
    expect_that!(subscription.notification().output().last_success, none());
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_sentinel_id)
        .send()
        .await?;
    expect_that!(subscription_2.notification().output().times_sent, some(ge(3)));
    expect_that!(subscription_2.notification().output().last_notification, some(anything()));
    broker.subscriptions().delete(&subscription_sentinel_id).send().await?;
    broker.subscriptions().delete(&subscription_silent_id).send().await?;
    Ok(())
}
