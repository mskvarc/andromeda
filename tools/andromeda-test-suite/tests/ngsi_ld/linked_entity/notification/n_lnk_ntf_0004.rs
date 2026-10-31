//! A periodic subscription q hop carries only the entities it admits.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A periodic notification of a subscription whose q is isParkedIn{category}=="commercial" carries the car
/// parked in the commercial parking and not the one parked in the residential parking
/// Clause: 4.9, 5.8.6
#[gtest]
#[tokio::test]
async fn n_lnk_ntf_0004_a_periodic_subscription_q_hop_carries_only_the_entities_it_admits() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let parking_commercial_id = namespace.urn("Parking", "commercial")?;
    let parking_type = namespace.entity_type("Parking")?;
    let parking_residential_id = namespace.urn("Parking", "residential")?;
    let car_elsewhere_id = namespace.urn("Car", "elsewhere")?;
    let car_type = namespace.entity_type("Car")?;
    let car_parked_id = namespace.urn("Car", "parked")?;
    let subscription_id = namespace.subscription_id(1)?;
    let parking_commercial = Entity::builder()
        .id(parking_commercial_id.clone())
        .entity_type(parking_type.clone())
        .property("category", "commercial")
        .build()?;
    broker.entities().create(&parking_commercial).send().await?.done()?;
    let parking_residential = Entity::builder()
        .id(parking_residential_id.clone())
        .entity_type(parking_type.clone())
        .property("category", "residential")
        .build()?;
    broker.entities().create(&parking_residential).send().await?.done()?;
    let car_elsewhere = Entity::builder()
        .id(car_elsewhere_id)
        .entity_type(car_type.clone())
        .relationship("isParkedIn", parking_residential_id.clone())
        .build()?;
    broker.entities().create(&car_elsewhere).send().await?.done()?;
    let car_parked = Entity::builder()
        .id(car_parked_id.clone())
        .entity_type(car_type.clone())
        .relationship("isParkedIn", parking_commercial_id.clone())
        .build()?;
    broker.entities().create(&car_parked).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([car_type.as_str()])
        .time_interval(2)
        .q(q::attr("isParkedIn").linked("category").eq("commercial").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), len(eq(1)));
    expect_that!(notification.data().as_slice(), contains(has_id(&car_parked_id)));
    Ok(())
}
