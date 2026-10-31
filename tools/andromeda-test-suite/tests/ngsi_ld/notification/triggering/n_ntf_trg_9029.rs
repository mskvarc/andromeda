//! A periodic notification goes out at its own grid point while another periodic subscription shares the tenant.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use chrono::Utc;
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, Lossless, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::time::Duration;
use tokio::time::sleep;

/// Pins: in a tenant of its own, a Subscription with `timeInterval` 1 created half a second after another one with
/// `timeInterval` 1 on the same Entity is notified within 200 ms after each of its own grid points, its `createdAt`
/// plus a whole number of seconds, and never at the other Subscription's
/// Clause: 5.8.6
/// Recorded: clause 5.8.6 notifies "when the time interval (in seconds) specified in such value field is reached",
/// so the grid of one Subscription is not delayed by the grid of another; 200 ms bounds one sweep pass
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9029_a_periodic_notification_goes_out_at_its_own_grid_point_beside_another_periodic_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let sink = NotificationSink::start().await?;
    let vehicle_id = namespace.urn("Vehicle", "grid")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let leading_id = namespace.subscription_id("leading")?;
    let offset_id = namespace.subscription_id("offset")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let selector = EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_id);
    let leading = Subscription::builder()
        .id(leading_id.clone())
        // clone: both Subscriptions select the same Entity
        .entities([selector.clone()])
        .time_interval(1)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("leading")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&leading).send().await?;
    sleep(Duration::from_millis(500)).await;
    let offset = Subscription::builder()
        .id(offset_id.clone())
        .entities([selector])
        .time_interval(1)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("offset")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&offset).send().await?;
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&offset_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let created_at = chrono::DateTime::<Utc>::from(stored.output().created_at.or_fail()?);
    let notifications = sink.wait_for_at_least("offset", 3).await?;
    for received in &notifications {
        let notification = received.entities(Lossless::Normalized)?;
        expect_that!(notification.subscription_id(), eq(&offset_id));
        let elapsed = (chrono::DateTime::<Utc>::from(*notification.notified_at()) - created_at).num_milliseconds();
        expect_that!(elapsed, ge(1_000));
        expect_that!(elapsed % 1_000, le(200));
    }
    broker.subscriptions().delete(&offset_id).send().await?;
    broker.subscriptions().delete(&leading_id).send().await?;
    Ok(())
}
