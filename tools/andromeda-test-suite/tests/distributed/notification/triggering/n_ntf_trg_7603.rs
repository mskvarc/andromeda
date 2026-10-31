//! A paused subscription is not notified until reactivated.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType, Representation},
    subscription::{Activation, Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Create Entity on broker A while a subscription created on broker A is paused notifies nothing; after the
/// subscription is reactivated, an update of the Entity is notified, and it is the only notification
/// Clause: 5.8.6, 5.2.12
#[gtest]
#[tokio::test]
async fn n_ntf_trg_7603_a_paused_subscription_is_not_notified_until_reactivated() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .activation(Activation::Paused)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 40)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = SubscriptionFragment::builder().activation(Activation::Active).build()?;
    broker.subscriptions().update(&subscription_id, &fragment).send().await?;
    let fragment_2 = EntityFragment::builder().property("speed", 41).build()?;
    broker.entities().update_attributes(&vehicle_id, &fragment_2).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_attribute("speed", any_attribute().value(41))));
    Ok(())
}
