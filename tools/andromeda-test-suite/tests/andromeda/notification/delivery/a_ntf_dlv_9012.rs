//! An endpoint that never answers leaves the other Subscriptions on the same Entities notified on time, while its
//! attempts wait out the request timeout.

use andromeda_test_suite::{broker, config::BrokerRole, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityId, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::{collections::HashMap, time::Duration};
use tokio::time::{Instant, interval};
use wiremock::ResponseTemplate;

/// How long the silent endpoint holds a request before it would answer, far past the broker's `request_timeout`.
const SILENCE: Duration = Duration::from_secs(120);

/// The batches of Trams written, one every `BATCH_INTERVAL`.
const BATCHES: u32 = 48;

/// The Trams each batch creates.
const BATCH_SIZE: u32 = 5;

/// The pause between two batches: 20 creations per second for 12 seconds, longer than the silent endpoint's first
/// attempts are held before the 10-second `request_timeout` ends them.
const BATCH_INTERVAL: Duration = Duration::from_millis(250);

/// How often the answering endpoint's notifications are read once every batch is written.
const READ_INTERVAL: Duration = Duration::from_millis(100);

/// How soon after its creation each Tram is notified to the answering endpoint: half the 10-second
/// `request_timeout`. Creations claimed only once the silent endpoint's attempts time out would wait up to the whole
/// timeout.
const NOTIFIED_WITHIN: Duration = Duration::from_secs(5);

/// The instant each Tram first appears in a notification to the answering endpoint, read now, taking in the
/// notifications past the first `read`.
async fn take_in(sink: &NotificationSink, read: &mut usize, notified: &mut HashMap<EntityId, Instant>) -> Result<()> {
    let received = sink.received("answering").await?;
    let now = Instant::now();
    for notification in received.iter().skip(*read) {
        for entity in notification.entities(Lossless::Normalized)?.data() {
            notified.entry(entity.id.clone()).or_insert(now);
        }
    }
    *read = received.len();
    Ok(())
}

/// Pins: Under durable delivery, beside a Subscription whose endpoint accepts the connection and never answers, a
/// Subscription on the same Trams whose endpoint answers at once is notified of each of 240 Trams created at 20 per
/// second within 5 seconds of its creation
/// Clause: 5.8.6
/// Tenant: private (the deliveries of one tenant share its 20 delivery slots, so the silent endpoint would hold back
/// the notifications of every other test in the shared tenant for as long as it is attempted)
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable) with its default 20 slots and 10-second request timeout, where an attempt waiting on an endpoint
/// holds its own slot while the deliveries committed meanwhile are claimed into the others
#[gtest]
#[tokio::test]
async fn a_ntf_dlv_9012_an_endpoint_that_never_answers_leaves_the_other_subscriptions_notified_while_its_attempts_wait_out_the_request_timeout() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let tenant = broker::private_tenant(&broker_durable, &namespace).await?;
    let sink = NotificationSink::start().await?;
    sink.answer("silent", ResponseTemplate::new(204).set_delay(SILENCE)).await?;
    let tram_type = namespace.entity_type("Tram")?;
    let subscription_silent_id = namespace.subscription_id("silent")?;
    let subscription_answering_id = namespace.subscription_id("answering")?;
    for (id, endpoint) in [(&subscription_silent_id, "silent"), (&subscription_answering_id, "answering")] {
        let subscription = Subscription::builder()
            .id(id.clone())
            .entities([tram_type.as_str()])
            .notification_triggers([NotificationTrigger::EntityCreated])
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(endpoint)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        tenant.subscriptions().create(&subscription).send().await?;
    }
    let mut created = HashMap::new();
    let mut notified = HashMap::new();
    let mut read = 0;
    let mut pacing = interval(BATCH_INTERVAL);
    for batch in 0..BATCHES {
        pacing.tick().await;
        let trams = (1..=BATCH_SIZE)
            .map(|n| {
                Ok(Entity::builder()
                    .id(namespace.urn("Tram", batch * BATCH_SIZE + n)?)
                    .entity_type(tram_type.clone())
                    .property("car", n)
                    .build()?)
            })
            .collect::<Result<Vec<_>>>()?;
        let written_at = Instant::now();
        tenant.entities().batch_create(&trams).send().await?.done()?;
        created.extend(trams.into_iter().map(|tram| (tram.id, written_at)));
        take_in(&sink, &mut read, &mut notified).await?;
    }
    let deadline = Instant::now() + NOTIFIED_WITHIN;
    let mut reading = interval(READ_INTERVAL);
    while notified.len() < created.len() && Instant::now() < deadline {
        reading.tick().await;
        take_in(&sink, &mut read, &mut notified).await?;
    }
    let mut unnotified = Vec::new();
    let mut late = Vec::new();
    for (id, written_at) in &created {
        match notified.get(id) {
            Some(notified_at) if notified_at.duration_since(*written_at) > NOTIFIED_WITHIN => late.push((id, notified_at.duration_since(*written_at))),
            Some(_) => {}
            None => unnotified.push(id),
        }
    }
    expect_that!(unnotified, is_empty());
    expect_that!(late, is_empty());
    tenant.subscriptions().delete(&subscription_silent_id).send().await?;
    tenant.subscriptions().delete(&subscription_answering_id).send().await?;
    Ok(())
}
