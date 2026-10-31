//! A respond async update notifies a live subscription once.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    respond_async::{async_applied, respond_async},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::BatchUpdate,
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::time::Duration;

/// How long the endpoint is watched for a notification beyond the three owed.
const QUIET_WINDOW: Duration = Duration::from_secs(1);

/// Pins: a batch update of three Entities answered with Preference-Applied: respond-async notifies a Subscription
/// watching them once per Entity, each notification carrying the value the update wrote
/// Clause: 5.8.6, 5.2.12
/// Recorded: Andromeda extension, not v1.9.1: Prefer: respond-async on batch operations (IETF RFC 7240 section
/// 4.1), whose owed changes a live-delivery broker queues as typed change events and notifies once its worker
/// drains them
#[gtest]
#[tokio::test]
async fn a_bat_upd_0014_a_respond_async_update_notifies_a_live_subscription_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let subscription_id = namespace.subscription_id(1)?;
    let sensor_ids = (1..=3).map(|n| namespace.urn("Sensor", n)).collect::<Result<Vec<_>, _>>()?;
    let sensors = sensor_ids
        .iter()
        .map(|id| Entity::builder().id(id.clone()).entity_type(sensor_type.clone()).property("level", 1).build())
        .collect::<Result<Vec<_>, _>>()?;
    broker.entities().batch_create(&sensors).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([sensor_type.as_str()])
        .watched_attributes(["level"])
        .notification_triggers([NotificationTrigger::AttributeUpdated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragments = sensor_ids
        .iter()
        .map(|id| EntityFragment::for_entity(id.clone(), sensor_type.clone()).property("level", 2).build())
        .collect::<Result<Vec<_>, _>>()?;
    let request = respond_async(broker.unchecked(&BatchUpdate::builder().fragments(&fragments).build())?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, async_applied(StatusCode::NO_CONTENT));
    sink.wait_for_at_least(1, 3).await?;
    // Clause 5.8.6 notifies each change once: three Entities changed, three notifications.
    expect_that!(sink.count_after(1, QUIET_WINDOW).await?, eq(3));
    let notifications = sink.received(1).await?;
    for notification in &notifications {
        let notified = notification.entities(Lossless::Normalized)?;
        expect_that!(notified.data(), len(eq(1)));
        expect_that!(notified.data(), each(has_attribute("level", any_attribute().value(2))));
    }
    Ok(())
}
