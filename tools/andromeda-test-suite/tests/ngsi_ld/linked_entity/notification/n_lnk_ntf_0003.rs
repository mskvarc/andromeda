//! A subscription q with two hops reads the entity two relationships away.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: A subscription whose q is isParkedIn{locatedIn{zone}}=="north" is not notified of a car whose parking lies
/// in the south district and is notified of a car whose parking lies in the north district
/// Clause: 4.9, 5.8.6
#[gtest]
#[tokio::test]
async fn n_lnk_ntf_0003_a_subscription_q_with_two_hops_reads_the_entity_two_relationships_away() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let district_north_id = namespace.urn("District", "north")?;
    let district_type = namespace.entity_type("District")?;
    let parking_north_id = namespace.urn("Parking", "north")?;
    let parking_type = namespace.entity_type("Parking")?;
    let district_south_id = namespace.urn("District", "south")?;
    let parking_south_id = namespace.urn("Parking", "south")?;
    let subscription_id = namespace.subscription_id(1)?;
    let car_type = namespace.entity_type("Car")?;
    let car_south_id = namespace.urn("Car", "south")?;
    let car_north_id = namespace.urn("Car", "north")?;
    let district_north = Entity::builder()
        .id(district_north_id.clone())
        .entity_type(district_type.clone())
        .property("zone", "north")
        .build()?;
    broker.entities().create(&district_north).send().await?.done()?;
    let parking_north = Entity::builder()
        .id(parking_north_id.clone())
        .entity_type(parking_type.clone())
        .relationship("locatedIn", district_north_id.clone())
        .build()?;
    broker.entities().create(&parking_north).send().await?.done()?;
    let district_south = Entity::builder()
        .id(district_south_id.clone())
        .entity_type(district_type.clone())
        .property("zone", "south")
        .build()?;
    broker.entities().create(&district_south).send().await?.done()?;
    let parking_south = Entity::builder()
        .id(parking_south_id.clone())
        .entity_type(parking_type.clone())
        .relationship("locatedIn", district_south_id.clone())
        .build()?;
    broker.entities().create(&parking_south).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([car_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .q(q::attr("isParkedIn").linked("locatedIn").linked("zone").eq("north").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let car_south = Entity::builder()
        .id(car_south_id)
        .entity_type(car_type.clone())
        .relationship("isParkedIn", parking_south_id.clone())
        .build()?;
    broker.entities().create(&car_south).send().await?.done()?;
    let car_north = Entity::builder()
        .id(car_north_id.clone())
        .entity_type(car_type.clone())
        .relationship("isParkedIn", parking_north_id.clone())
        .build()?;
    broker.entities().create(&car_north).send().await?.done()?;
    // One notification, of the car created second: the first sent none.
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&car_north_id)));
    Ok(())
}
