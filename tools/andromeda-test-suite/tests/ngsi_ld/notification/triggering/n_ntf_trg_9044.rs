//! A subscription whose entity selector is a type expression is notified of exactly the Entities it selects.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityId, Lossless, NgsiLdMediaType, Representation},
    query::{EntitySelector, SelectorType, type_selection},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// The identifiers of the Entities every notification endpoint `n` of `sink` received names.
async fn notified(sink: &NotificationSink, n: u32) -> Result<Vec<EntityId>> {
    let mut notified = Vec::new();
    for received in sink.received(n).await? {
        notified.extend(received.entities(Lossless::Normalized)?.into_data().into_iter().map(|entity| entity.id));
    }
    Ok(notified)
}

/// Pins: a subscription on `Building|Tower` is notified of the creation of a Tower and of a Building and not of a
/// Vehicle, and one on `(Home;Vehicle)` is notified of an Entity typed both Home and Vehicle and not of a Vehicle or
/// a Home alone
/// Clause: 4.17, 5.2.33, 5.8.6
///
/// Clause 4.17 selects with `|` an Entity of either type and with `;` an Entity of every type. Each Entity carries an
/// attribute: without `notificationTrigger` the default triggers are `attributeCreated` and `attributeUpdated`
/// (Table 5.2.12-1), which an attribute-less creation does not raise.
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9044_a_subscription_on_a_type_expression_is_notified_of_the_entities_it_selects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building = namespace.entity_type("Building")?;
    let tower = namespace.entity_type("Tower")?;
    let home = namespace.entity_type("Home")?;
    let vehicle = namespace.entity_type("Vehicle")?;
    let selections = [
        (1, type_selection::entity_type(&building).or(&tower).build()?),
        (2, type_selection::all_of([&home, &vehicle]).build()?),
    ];
    for (n, selection) in selections {
        let subscription = Subscription::builder()
            .id(namespace.subscription_id(n)?)
            .entities([EntitySelector::new(SelectorType::Selection(selection))])
            .notification(
                NotificationParams::builder(Endpoint::builder(sink.endpoint(n)?).accept(NgsiLdMediaType::Json).build()?)
                    .format(Representation::Normalized)
                    .build()?,
            )
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
    }
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let home_id = namespace.urn("Home", 2)?;
    let tower_id = namespace.urn("Tower", 3)?;
    let building_id = namespace.urn("Building", 4)?;
    let motorhome_id = namespace.urn("Motorhome", 5)?;
    // The Entities neither selection selects are created first, so a notification of one would be the oldest its
    // endpoint holds.
    for (id, types) in [
        (&vehicle_id, vec![vehicle.clone()]),
        (&home_id, vec![home.clone()]),
        (&tower_id, vec![tower.clone()]),
        (&building_id, vec![building.clone()]),
        (&motorhome_id, vec![home.clone(), vehicle.clone()]),
    ] {
        let entity = Entity::builder().id(id.clone()).entity_type(types).property("name", "typed").build()?;
        broker.entities().create(&entity).send().await?.done()?;
    }
    // Notifications of two Entities arrive in either order.
    sink.wait_for_at_least(1, 2).await?;
    sink.wait_for_at_least(2, 1).await?;
    let notified_1 = notified(&sink, 1).await?;
    expect_that!(notified_1, len(eq(2)));
    expect_that!(notified_1.as_slice(), contains(eq(&tower_id)));
    expect_that!(notified_1.as_slice(), contains(eq(&building_id)));
    verify_that!(notified(&sink, 2).await?, elements_are![eq(&motorhome_id)])
}
