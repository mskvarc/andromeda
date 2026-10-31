//! A subscription q hop follows a relationship moved to another entity.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: A subscription whose q is isParkedIn{category}=="commercial" is not notified of the creation of a car
/// parked in a residential parking and is notified when an update moves its `isParkedIn` to the commercial parking
/// Clause: 4.9, 5.8.6
#[gtest]
#[tokio::test]
async fn n_lnk_ntf_0001_a_subscription_q_hop_follows_a_relationship_moved_to_another_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let parking_commercial_id = namespace.urn("Parking", "commercial")?;
    let parking_type = namespace.entity_type("Parking")?;
    let parking_residential_id = namespace.urn("Parking", "residential")?;
    let subscription_id = namespace.subscription_id(1)?;
    let car_type = namespace.entity_type("Car")?;
    let car_moving_id = namespace.urn("Car", "moving")?;
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
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([car_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated, NotificationTrigger::AttributeUpdated])
        .q(q::attr("isParkedIn").linked("category").eq("commercial").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let car_moving = Entity::builder()
        .id(car_moving_id.clone())
        .entity_type(car_type.clone())
        .relationship("isParkedIn", parking_residential_id.clone())
        .build()?;
    broker.entities().create(&car_moving).send().await?.done()?;
    let fragment = EntityFragment::builder().relationship("isParkedIn", parking_commercial_id.clone()).build()?;
    broker.entities().update_attributes(&car_moving_id, &fragment).send().await?.done()?;
    // One notification, of the update: the creation sent none.
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            all!(
                has_id(&car_moving_id),
                has_attribute("isParkedIn", any_attribute().object(parking_commercial_id.clone()))
            )
        )
    );
    Ok(())
}
