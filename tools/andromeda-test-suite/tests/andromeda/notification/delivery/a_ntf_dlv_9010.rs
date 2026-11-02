//! The deliveries queued while an endpoint is held are each sent once it recovers.

use andromeda_test_suite::{broker, config::BrokerRole, eventually::retried, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::time::Duration;

/// The Kiosks created once the endpoint has failed, more than the 20 deliveries a drain sends at once.
const QUEUED_WHILE_HELD: u32 = 30;

/// How long the endpoint is watched for a notification beyond the ones owed.
const QUIET_WINDOW: Duration = Duration::from_secs(3);

/// Pins: Under durable delivery, once an endpoint that closes every connection unanswered has failed, the 30
/// notifications queued for it afterwards and the one that failed are each received once, for each created Kiosk, once
/// the endpoint answers
/// Clause: 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable), where a delivery queued for a Subscription held behind its probe is queued set aside and an
/// attempt the endpoint answers releases it
#[gtest]
#[tokio::test]
async fn a_ntf_dlv_9010_the_deliveries_queued_while_an_endpoint_is_held_are_each_sent_once_it_recovers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let sink = NotificationSink::refusing()?;
    let kiosk_type = namespace.entity_type("Kiosk")?;
    let subscription_id = namespace.subscription_id("held")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([kiosk_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("held")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker_durable.subscriptions().create(&subscription).send().await?;
    let kiosk_ids = (0..=QUEUED_WHILE_HELD).map(|n| namespace.urn("Kiosk", n)).collect::<Result<Vec<_>, _>>()?;
    let kiosks = kiosk_ids
        .iter()
        .map(|id| Entity::builder().id(id.clone()).entity_type(kiosk_type.clone()).property("open", true).build())
        .collect::<Result<Vec<_>, _>>()?;
    let (probe, queued_while_held) = kiosks.split_at(1);
    broker_durable.entities().batch_create(probe).send().await?.done()?;
    retried(async || {
        let stored = broker_durable
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        verify_that!(stored.notification().output().times_failed, some(ge(1)))
    })
    .await?;
    broker_durable.entities().batch_create(queued_while_held).send().await?.done()?;
    let sink = sink.start_serving().await?;
    sink.wait_for_at_least("held", kiosk_ids.len()).await?;
    expect_that!(sink.count_after("held", QUIET_WINDOW).await?, eq(kiosk_ids.len()));
    let mut notified = Vec::new();
    for notification in sink.received("held").await? {
        notified.extend(notification.entities(Lossless::Normalized)?.data().iter().map(|entity| entity.id.to_string()));
    }
    notified.sort();
    let mut created: Vec<String> = kiosk_ids.iter().map(ToString::to_string).collect();
    created.sort();
    expect_that!(notified, eq(&created));
    broker_durable.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
