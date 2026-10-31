//! A deleted subscription notifies nothing further.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Delete Subscription answers 204, and an entity created afterwards on the subscribed type is notified to a
/// control subscription and not to the deleted one
/// Clause: 5.8.5.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_sub_del_6801_a_deleted_subscription_notifies_nothing_further() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 10)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    broker.subscriptions().delete(&subscription_id).send().await?;
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id)
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_2).send().await?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 20)
        .build()?;
    broker.entities().create(&vehicle_2).send().await?.done()?;
    let notifications_2 = sink.wait_for_at_least(2, 1).await?;
    expect_that!(notifications_2, len(eq(1)));
    let notifications_3 = sink.received(1).await?;
    expect_that!(notifications_3, len(eq(1)));
    Ok(())
}
