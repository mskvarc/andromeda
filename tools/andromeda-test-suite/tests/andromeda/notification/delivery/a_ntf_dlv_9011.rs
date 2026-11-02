//! Every notification of a busy Subscription is counted, and the counters read back at once.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    eventually::{Poll, eventually},
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::time::Duration;

/// The batches of Lamps written back to back.
const BATCHES: u32 = 8;

/// The Lamps each batch creates: 200 in all, enough to keep the 20 delivery slots busy with this one Subscription.
const BATCH_SIZE: u32 = 25;

/// How soon after the last notification arrives its outcome reads back on the Subscription.
const RECORDED_WITHIN: Duration = Duration::from_secs(1);

/// Pins: Under durable delivery, a Subscription notified of 200 Lamps created at once, by an endpoint that answers
/// each at once, reads back `timesSent` 200 with `lastNotification` and `lastSuccess` set and no failure within 1
/// second of the last notification arriving
/// Clause: 5.2.14.2, 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable), where the outcomes of one Subscription's notifications in flight together are recorded by one
/// write of its delivery counters
#[gtest]
#[tokio::test]
async fn a_ntf_dlv_9011_every_notification_of_a_busy_subscription_is_counted_and_the_counters_read_back_at_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let sink = NotificationSink::start().await?;
    let lamp_type = namespace.entity_type("Lamp")?;
    let subscription_id = namespace.subscription_id("busy")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([lamp_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("busy")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker_durable.subscriptions().create(&subscription).send().await?;
    for batch in 0..BATCHES {
        let lamps = (1..=BATCH_SIZE)
            .map(|n| {
                Ok(Entity::builder()
                    .id(namespace.urn("Lamp", batch * BATCH_SIZE + n)?)
                    .entity_type(lamp_type.clone())
                    .property("lit", true)
                    .build()?)
            })
            .collect::<Result<Vec<_>>>()?;
        broker_durable.entities().batch_create(&lamps).send().await?.done()?;
    }
    let created = usize::try_from(BATCHES * BATCH_SIZE)?;
    sink.wait_for_at_least("busy", created).await?;
    let reader = broker_durable.with_accept(Accept::new(NgsiLdMediaType::Json));
    let stored = eventually(RECORDED_WITHIN, || async {
        match reader.subscriptions().retrieve(&subscription_id).send().await {
            Ok(stored) if stored.notification().output().times_sent == Some(u64::from(BATCHES * BATCH_SIZE)) => Poll::Ready(stored),
            Ok(stored) => Poll::Pending(format!("timesSent {:?}", stored.notification().output().times_sent)),
            Err(error) => Poll::Pending(format!("the retrieval failed: {error}")),
        }
    })
    .await?;
    expect_that!(stored.notification().output().last_notification, some(anything()));
    expect_that!(stored.notification().output().last_success, some(anything()));
    expect_that!(stored.notification().output().times_failed, any![none(), some(eq(0))]);
    expect_that!(stored.notification().output().last_failure, none());
    broker_durable.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
