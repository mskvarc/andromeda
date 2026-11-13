//! A periodic notification leaves out an entity past its expires at.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    instant_ahead::minutes_ahead,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{DateTime, Entity, Lossless, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription with `timeInterval` 1 and q speed>10, created right after two Vehicles with speed 20, one
/// whose `expiresAt` passed a minute ago, notifies first with the other Vehicle alone
/// Clause: 4.22, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9035_a_periodic_notification_leaves_out_an_entity_past_its_expires_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_expired_id = namespace.urn("Vehicle", "expired")?;
    let vehicle_standing_id = namespace.urn("Vehicle", "standing")?;
    let subscription_id = namespace.subscription_id("periodic")?;
    let a_minute_ago: DateTime = minutes_ahead(-1).parse()?;
    let vehicle_expired = Entity::builder()
        .id(vehicle_expired_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 20)
        .expires_at(a_minute_ago)
        .build()?;
    broker.entities().create(&vehicle_expired).send().await?.done()?;
    let vehicle_standing = Entity::builder()
        .id(vehicle_standing_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 20)
        .build()?;
    broker.entities().create(&vehicle_standing).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .time_interval(1)
        .q(q::attr("speed").gt(10).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("periodic")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let notifications = sink.wait_for_at_least("periodic", 1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), elements_are![has_id(&vehicle_standing_id)]);
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
