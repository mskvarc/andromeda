//! A creation notifies a subscription without watched attributes.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Entity on broker A holding only colour notifies a subscription created on broker A with no
/// `watchedAttributes`, as a Notification with a URI id, the subscription id, `notifiedAt` and the Entity in data
/// Clause: 5.8.6, 5.2.25, 5.2.12
#[gtest]
#[tokio::test]
async fn n_ntf_trg_7600_a_creation_notifies_a_subscription_without_watched_attributes() -> Result<()> {
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
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("colour", "red")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.id().to_string(), contains_regex("^[A-Za-z][A-Za-z0-9+.-]*:.+"));
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.notified_at().to_string(), contains_regex("^20[0-9][0-9]-"));
    expect_that!(notification.data(), len(eq(1)));
    let notified = notification.data().first().or_fail()?;
    expect_that!(notified, has_id(&vehicle_id));
    expect_that!(notified, has_attribute("colour", any_attribute().value("red")));
    Ok(())
}
