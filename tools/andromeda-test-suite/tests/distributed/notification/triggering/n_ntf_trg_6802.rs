//! A periodic notification carries the entities the q selects and nothing when none match.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Of two entities, a periodic subscription with q speed>50 notifies the fast one alone, one without q
/// notifies both, and one whose q selects none has received nothing when the other has
/// Clause: 5.8.6, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_ntf_trg_6802_a_periodic_notification_carries_the_entities_the_q_selects_and_nothing_when_none_match() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_fast_id = namespace.urn("Vehicle", "fast")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_slow_id = namespace.urn("Vehicle", "slow")?;
    let subscription_3_id = namespace.subscription_id(3)?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let vehicle_fast = Entity::builder()
        .id(vehicle_fast_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 100)
        .build()?;
    broker.entities().create(&vehicle_fast).send().await?.done()?;
    let vehicle_slow = Entity::builder()
        .id(vehicle_slow_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 20)
        .build()?;
    broker.entities().create(&vehicle_slow).send().await?.done()?;
    let subscription_3 = Subscription::builder()
        .id(subscription_3_id)
        .entities([vehicle_type.as_str()])
        .time_interval(1)
        .q(q::attr("speed").gt(9999).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(3)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_3).send().await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .time_interval(1)
        .q(q::attr("speed").gt(50).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id)
        .entities([vehicle_type.as_str()])
        .time_interval(1)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_2).send().await?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), contains(has_id(&vehicle_fast_id)));
    expect_that!(notification.data().as_slice(), not(contains(has_id(&vehicle_slow_id))));
    let notifications_2 = sink.wait_for_at_least(2, 1).await?;
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.data().as_slice(), contains(has_id(&vehicle_fast_id)));
    expect_that!(notification_2.data().as_slice(), contains(has_id(&vehicle_slow_id)));
    let notifications_3 = sink.received(3).await?;
    expect_that!(notifications_3, len(eq(0)));
    Ok(())
}
