//! A pick reduces the notified entity to the picked members.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    query::projection::Projection,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A notification of a subscription whose notification picks speed carries speed and not the entity colour
/// Clause: 5.8.6, 4.21
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6817_a_pick_reduces_the_notified_entity_to_the_picked_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .pick("speed".parse::<Projection>()?)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let vehicle = Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type(vehicle_type)
        .property("speed", 20)
        .property("colour", "red")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.projected_entities(Lossless::Normalized)?;
    expect_that!(
        notification.data(),
        nth(0, all!(has_attribute("speed", any_attribute().value(20)), lacks_attribute("colour")))
    );
    Ok(())
}
