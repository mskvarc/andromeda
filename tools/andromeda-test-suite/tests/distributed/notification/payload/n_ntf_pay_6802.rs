//! A concise notification drops the property type and keeps value with its sub attributes.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_json::{json_absent, json_equals},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType, Property, Representation, Value},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A notification of a subscription whose format is concise carries a Property with a sub-Property as an
/// object holding value and the bare sub-attribute, with no type
/// Clause: 5.8.6, 4.5.2.3
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6802_a_concise_notification_drops_the_property_type_and_keeps_value_with_its_sub_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Concise)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let vehicle = Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type(vehicle_type)
        .property("speed", Property::builder(20).property("accuracy", Value::Number("0.5".parse()?)))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let received = notifications.first().or_fail()?;
    let notification = received.entities(Lossless::Concise)?;
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            has_attribute(
                "speed",
                any_attribute()
                    .value(20)
                    .sub_attribute("accuracy", any_attribute().value(Value::Number("0.5".parse()?)))
            )
        )
    );
    // The concise form drops `type`, which the typed read restores, so the absence is checked on the notification as received.
    expect_that!(&received.body, json_equals("$.data[0].speed.value", 20));
    expect_that!(&received.body, json_absent("$.data[0].speed.type"));
    expect_that!(&received.body, json_equals("$.data[0].speed.accuracy", 0.5));
    Ok(())
}
