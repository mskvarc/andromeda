//! A delivered notification is counted and reported ok.

use andromeda_test_suite::{broker, eventually::retried, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, NotificationStatus, Subscription},
};

/// Pins: after a notification the endpoint answers with 200, the subscription reads back with `timesSent` counting
/// it, `lastNotification` and `lastSuccess` stamped and notification status ok
/// Clause: 5.8.6, 5.2.14.2, 5.8.3
#[gtest]
#[tokio::test]
async fn n_ntf_sta_0003_a_delivered_notification_is_counted_and_reported_ok() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let building = Entity::builder().id(building_id).entity_type(building_type).property("name", "Hall").build()?;
    broker.entities().create(&building).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    retried(async || {
        let subscription = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        let notification = subscription.notification().output();
        verify_that!(notification.times_sent, some(ge(1)))?;
        verify_that!(notification.last_notification, some(anything()))?;
        verify_that!(notification.last_success, some(anything()))?;
        verify_that!(notification.status, some(eq(NotificationStatus::Ok)))
    })
    .await?;
    Ok(())
}
