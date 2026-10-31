//! Without notification attributes every attribute is notified.

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
    model::{Entity, Lossless, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A notification of a creation on broker A holding speed and colour, to a subscription created on broker A
/// without notification attributes, carries both
/// Clause: 5.8.6, 5.2.14
#[gtest]
#[tokio::test]
async fn n_ntf_pay_7602_without_notification_attributes_every_attribute_is_notified() -> Result<()> {
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
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 20)
        .property("colour", "red")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            all!(
                has_attribute("speed", any_attribute().value(20)),
                has_attribute("colour", any_attribute().value("red"))
            )
        )
    );
    Ok(())
}
