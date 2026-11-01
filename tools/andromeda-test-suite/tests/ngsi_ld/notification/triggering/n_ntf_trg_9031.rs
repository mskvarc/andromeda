//! A subscription q reads a value's data type from its declared datatype.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    value::xsd_typed,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType, Value},
    query::q,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: a subscription whose q is reading==4 is not notified of an Entity whose `reading` is the plain JSON string
/// "4" and is notified of one whose `reading` is "4" declared xsd:integer
/// Clause: 4.9, 5.8.6, C.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9031_a_subscription_q_reads_the_declared_datatype() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_plain_id = namespace.urn("Sensor", "plain")?;
    let sensor_declared_id = namespace.urn("Sensor", "declared")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([sensor_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .q(q::attr("reading").eq(4).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let sensor_plain = Entity::builder()
        .id(sensor_plain_id)
        .entity_type(sensor_type.clone())
        .property("reading", Value::from("4"))
        .build()?;
    broker.entities().create(&sensor_plain).send().await?.done()?;
    let sensor_declared = Entity::builder()
        .id(sensor_declared_id.clone())
        .entity_type(sensor_type.clone())
        .property("reading", xsd_typed("integer", "4")?)
        .build()?;
    broker.entities().create(&sensor_declared).send().await?.done()?;
    // One notification, of the Entity created second: the plain string is no number.
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&sensor_declared_id)));
    Ok(())
}
