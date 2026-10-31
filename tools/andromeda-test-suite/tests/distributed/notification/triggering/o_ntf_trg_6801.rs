//! Rewriting a watched attribute with its stored value notifies nothing.

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
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Update Attributes writing the value an attribute already holds answers 204 and notifies no subscription, a
/// later write of a different value is notified, and the sink then holds that one notification
/// Clause: 5.8.6
/// Recorded: 5.8.6 sends a notification whenever a watched Attribute changes and does not say whether rewriting the
/// value it already holds is a change
#[gtest]
#[tokio::test]
async fn o_ntf_trg_6801_rewriting_a_watched_attribute_with_its_stored_value_notifies_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .watched_attributes(["speed"])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = EntityFragment::builder().property("speed", 50).build()?;
    broker.entities().update_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let fragment_2 = EntityFragment::builder().property("speed", 70).build()?;
    broker.entities().update_attributes(&vehicle_id, &fragment_2).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_attribute("speed", any_attribute().value(70))));
    let notifications_2 = sink.received(1).await?;
    expect_that!(notifications_2, len(eq(1)));
    Ok(())
}
