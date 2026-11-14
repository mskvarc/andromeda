//! A subscription selecting an id array notifies a change of a listed entity.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType, OneOrMany},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a Subscription whose one selector names a Vehicle type and the id array of the second and the first Vehicle
/// is created, and notifies the creation of the first Vehicle and not that of a third Vehicle created before it; an
/// `EntitySelector` `id` is a String or an array of Strings
/// Clause: 5.2.33, 5.8.1.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9037_a_subscription_selecting_an_id_array_notifies_a_change_of_a_listed_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let first_id = namespace.urn("Vehicle", 1)?;
    let second_id = namespace.urn("Vehicle", 2)?;
    let third_id = namespace.urn("Vehicle", 3)?;
    let subscription_id = namespace.subscription_id("id-array")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(OneOrMany::try_from(vec![second_id, first_id.clone()])?)])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    // The third Vehicle is created first, so a notification of it would precede the first Vehicle's.
    for id in [&third_id, &first_id] {
        let vehicle = Entity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 50)
            .build()?;
        broker.entities().create(&vehicle).send().await?.done()?;
    }
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), len(eq(1)));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&first_id)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
