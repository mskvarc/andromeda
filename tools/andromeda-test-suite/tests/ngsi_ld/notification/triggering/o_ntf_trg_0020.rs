//! The notifications of each entity follow the order of its updates.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Complete, Entity, EntityFragment, Input, Lossless, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::ops::RangeInclusive;

/// The Vehicles every Batch Entity Update sets `speed` on, by number.
const VEHICLES: RangeInclusive<i32> = 1..=8;

/// The `speed` each Batch Entity Update sets, in the order the updates are sent.
const UPDATES: RangeInclusive<i32> = 1..=8;

/// Pins: Eight Batch Entity Updates in turn, each setting speed on the same eight Entities, notify a Subscription
/// watching speed separately of every update, and the notifications of each Entity arrive in the order its updates
/// were committed
/// Clause: 5.5.11.3, 5.8.6
/// Recorded: 5.5.11.3 requires a separate notification for each individual update and 5.8.6 one for each change of
/// a watched attribute, but no clause orders notifications; Andromeda delivers the notifications of one Entity in
/// the order its changes were committed and lets those of different Entities overtake each other
#[gtest]
#[tokio::test]
async fn o_ntf_trg_0020_the_notifications_of_each_entity_follow_the_order_of_its_updates() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_ordered_id = namespace.subscription_id("ordered")?;
    let entities = VEHICLES
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", n)?)
                .entity_type(vehicle_type.clone())
                .property("speed", 0)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    let subscription_ordered = Subscription::builder()
        .id(subscription_ordered_id.clone())
        .entities([vehicle_type.as_str()])
        .watched_attributes(["speed"])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("ordered")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_ordered).send().await?;
    for speed in UPDATES {
        let fragments = VEHICLES
            .map(|n| -> Result<EntityFragment<Complete>> {
                Ok(EntityFragment::for_entity(namespace.urn("Vehicle", n)?, vehicle_type.clone())
                    .property("speed", speed)
                    .build()?)
            })
            .collect::<Result<Vec<_>>>()?;
        broker.entities().batch_update(&fragments).send().await?.done()?;
    }
    for n in VEHICLES {
        let selected = [BodyPattern::Contains(format!("\"urn:ngsi-ld:Vehicle:{namespace}-{n}\""))];
        let notifications = sink.wait_for_at_least_where("ordered", &selected, 8).await?;
        expect_that!(notifications, len(eq(8)));
        for (position, speed) in UPDATES.enumerate() {
            let notification = notifications.get(position).or_fail()?.entities(Lossless::Normalized)?;
            expect_that!(notification.data().as_slice(), nth(0, has_attribute("speed", any_attribute().value(speed))));
        }
    }
    let every_notification = sink.received("ordered").await?;
    expect_that!(every_notification, len(eq(64)));
    broker.subscriptions().delete(&subscription_ordered_id).send().await?;
    Ok(())
}
