//! An ld json notification and each entity of its data open with the context.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A notification sent with accept application/ld+json opens with @context, and so does the entity of its
/// data
/// Clause: 6.3.6, 5.3.1, 5.8.6
/// Recorded: 6.3.6 requires the @context in an application/ld+json body and fixes no member order (RFC 8259 objects
/// are unordered); Andromeda writes it as the first member, of every item of a collection too
#[gtest]
#[tokio::test]
async fn o_ntf_pay_9002_an_ld_json_notification_and_each_entity_of_its_data_open_with_the_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::JsonLd).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let vehicle = Entity::builder().id(vehicle_id).entity_type(vehicle_type).property("speed", 7).build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let text = String::from_utf8_lossy(&notifications.first().or_fail()?.body).into_owned();
    expect_that!(text, contains_regex(r#"^\s*\{\s*"@context""#));
    expect_that!(text, contains_regex(r#""data"\s*:\s*\[\s*\{\s*"@context""#));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
