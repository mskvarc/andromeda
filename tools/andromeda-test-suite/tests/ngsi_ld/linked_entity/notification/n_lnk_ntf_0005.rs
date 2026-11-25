//! A subscription q hop with a list of one type notifies only through a linked Entity of that type.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: a Subscription to Cars with q isParkedIn{Building:category}=="commercial" is notified of the creation of a
/// Car parked in a commercial Building and not of the creation of a Car parked in a commercial Warehouse
/// Clause: 4.9, 5.8.6
#[gtest]
#[tokio::test]
async fn n_lnk_ntf_0005_a_subscription_q_hop_with_a_list_of_one_type_notifies_only_through_a_linked_entity_of_that_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_id = namespace.urn("Building", "commercial")?;
    let building_type = namespace.entity_type("Building")?;
    let warehouse_id = namespace.urn("Warehouse", "commercial")?;
    let car_type = namespace.entity_type("Car")?;
    let car_in_building_id = namespace.urn("Car", "in-building")?;
    let subscription_id = namespace.subscription_id(1)?;
    let parkings = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("category", "commercial")
            .build()?,
        Entity::builder()
            .id(warehouse_id.clone())
            .entity_type(namespace.entity_type("Warehouse")?)
            .property("category", "commercial")
            .build()?,
    ];
    broker.entities().batch_create(&parkings).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([car_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .q(q::attr("isParkedIn").linked_typed([&building_type], "category").eq("commercial").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let car_in_warehouse = Entity::builder()
        .id(namespace.urn("Car", "in-warehouse")?)
        .entity_type(car_type.clone())
        .relationship("isParkedIn", warehouse_id)
        .build()?;
    broker.entities().create(&car_in_warehouse).send().await?.done()?;
    let car_in_building = Entity::builder()
        .id(car_in_building_id.clone())
        .entity_type(car_type)
        .relationship("isParkedIn", building_id)
        .build()?;
    broker.entities().create(&car_in_building).send().await?.done()?;
    // One notification, of the second creation: the Car parked in the Warehouse was created first and sent none.
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), len(eq(1)));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&car_in_building_id)));
    Ok(())
}
