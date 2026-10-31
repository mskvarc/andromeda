//! Notified at carries at most six fractional digits.

use andromeda_test_suite::{
    body_json::{json_equals, json_matches},
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: The `notifiedAt` of a Notification carries at most six fractional-second digits and ends in Z
/// Clause: Table 5.3.1-1, 4.6.3
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9038_notified_at_carries_at_most_six_fractional_digits() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let vehicle = Entity::builder().id(vehicle_id).entity_type(vehicle_type).property("speed", 1).build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let body = &notifications.first().or_fail()?.body;
    // The spelling of `notifiedAt` is the subject, which the model reads into an instant, so it is read as received.
    expect_that!(body, json_equals("$.type", "Notification"));
    expect_that!(body, json_matches("$.notifiedAt", r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{1,6})?Z$")?);
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
