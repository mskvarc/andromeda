//! A subscription q hop with entity types reads only linked entities of those types.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: A subscription whose q is isParkedIn{Garage,Depot:category}=="commercial" is not notified of a car parked
/// in a commercial Parking and is notified of a car parked in a commercial Garage
/// Clause: 4.9, 5.8.6
#[gtest]
#[tokio::test]
async fn n_lnk_ntf_0002_a_subscription_q_hop_with_entity_types_reads_only_linked_entities_of_those_types() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let parking_commercial_id = namespace.urn("Parking", "commercial")?;
    let parking_type = namespace.entity_type("Parking")?;
    let parking_residential_id = namespace.urn("Parking", "residential")?;
    let garage_id = namespace.urn("Garage", 1)?;
    let garage_type = namespace.entity_type("Garage")?;
    let subscription_id = namespace.subscription_id(1)?;
    let car_type = namespace.entity_type("Car")?;
    let car_in_parking_id = namespace.urn("Car", "in-parking")?;
    let car_in_garage_id = namespace.urn("Car", "in-garage")?;
    let parking_commercial = Entity::builder()
        .id(parking_commercial_id.clone())
        .entity_type(parking_type.clone())
        .property("category", "commercial")
        .build()?;
    broker.entities().create(&parking_commercial).send().await?.done()?;
    let parking_residential = Entity::builder()
        .id(parking_residential_id)
        .entity_type(parking_type.clone())
        .property("category", "residential")
        .build()?;
    broker.entities().create(&parking_residential).send().await?.done()?;
    let garage = Entity::builder()
        .id(garage_id.clone())
        .entity_type(garage_type.clone())
        .property("category", "commercial")
        .build()?;
    broker.entities().create(&garage).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([car_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .q(q::attr("isParkedIn")
            .linked_typed([&garage_type, &namespace.entity_type("Depot")?], "category")
            .eq("commercial")
            .build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let car_in_parking = Entity::builder()
        .id(car_in_parking_id)
        .entity_type(car_type.clone())
        .relationship("isParkedIn", parking_commercial_id.clone())
        .build()?;
    broker.entities().create(&car_in_parking).send().await?.done()?;
    let car_in_garage = Entity::builder()
        .id(car_in_garage_id.clone())
        .entity_type(car_type.clone())
        .relationship("isParkedIn", garage_id.clone())
        .build()?;
    broker.entities().create(&car_in_garage).send().await?.done()?;
    // One notification, of the car created second: the first sent none.
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&car_in_garage_id)));
    Ok(())
}
