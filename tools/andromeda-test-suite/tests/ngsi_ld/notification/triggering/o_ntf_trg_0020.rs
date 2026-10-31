//! Each update of an entity is notified separately.

use andromeda_test_suite::{body_pattern::BodyPattern, broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Complete, Entity, EntityFragment, Input, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::ops::RangeInclusive;

/// The Vehicles every Batch Entity Update sets `speed` on, by number.
const VEHICLES: RangeInclusive<i32> = 1..=8;

/// The `speed` each Batch Entity Update sets.
const UPDATES: RangeInclusive<i32> = 1..=8;

/// Pins: Eight Batch Entity Updates in turn, each setting speed on the same eight Entities, notify a Subscription
/// watching speed separately of every update: each Entity is notified once with each speed
/// Clause: 5.5.11.3, 5.8.6
/// Recorded: 5.5.11.3 requires a separate notification for each individual update and 5.8.6 one for each change of
/// a watched attribute, but no clause orders notifications, so the notifications are compared without regard to the
/// order they arrive in
#[gtest]
#[tokio::test]
async fn o_ntf_trg_0020_each_update_of_an_entity_is_notified_separately() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_every_id = namespace.subscription_id("every")?;
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
    let subscription_every = Subscription::builder()
        .id(subscription_every_id.clone())
        .entities([vehicle_type.as_str()])
        .watched_attributes(["speed"])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("every")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_every).send().await?;
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
        let notifications = sink.wait_for_at_least_where("every", &selected, 8).await?;
        expect_that!(notifications, len(eq(8)));
        for speed in UPDATES {
            let notified = [BodyPattern::Selects(
                format!("$.data[?(@.id == 'urn:ngsi-ld:Vehicle:{namespace}-{n}' && @.speed.value == {speed})]").parse()?,
            )];
            expect_that!(sink.received_where("every", &notified).await?, len(eq(1)));
        }
    }
    let every_notification = sink.received("every").await?;
    expect_that!(every_notification, len(eq(64)));
    broker.subscriptions().delete(&subscription_every_id).send().await?;
    Ok(())
}
