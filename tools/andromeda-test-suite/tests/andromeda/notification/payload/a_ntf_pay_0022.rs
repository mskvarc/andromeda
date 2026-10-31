//! A durable notification keeps a large number exact.

use andromeda_test_suite::{broker, config::BrokerRole, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, NgsiLdMediaType, Number, Property, Value},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use regex::Regex;

/// Pins: under durable delivery a notification carries a Property number past the double range written as the
/// client wrote it
/// Clause: 4.6.3
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable), where the queued event holds a number in an exact numeric field
#[gtest]
#[tokio::test]
async fn a_ntf_pay_0022_a_durable_notification_keeps_a_large_number_exact() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker_durable.subscriptions().create(&subscription).send().await?;
    // The value is past the double range, so the notification must carry it as the client wrote it.
    let building = Entity::builder()
        .id(namespace.urn("Building", "large")?)
        .entity_type(building_type.clone())
        .property("count", Property::builder(Value::Number("12345678901234567890".parse::<Number>()?)))
        .build()?;
    broker_durable.entities().create(&building).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(sink.received(1).await?, len(eq(1)));
    let body = String::from_utf8_lossy(&notifications.first().or_fail()?.body).into_owned();
    expect_that!(Regex::new(r#""value":12345678901234567890[,}]"#)?.is_match(&body), eq(true));
    Ok(())
}
