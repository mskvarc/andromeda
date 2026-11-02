//! An endpoint that answers slowly leaves the other Subscriptions of its tenant their delivery slots.

use andromeda_test_suite::{broker, config::BrokerRole, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::time::Duration;
use tokio::time::{interval, timeout};
use wiremock::ResponseTemplate;

/// How long the slow endpoint takes to answer each notification: within the 10-second `request_timeout`, so every
/// attempt succeeds and no hold is ever taken.
const ANSWER_DELAY: Duration = Duration::from_secs(2);

/// The batches of Trams written, one every `BATCH_INTERVAL`.
const BATCHES: u32 = 16;

/// The Trams each batch creates.
const BATCH_SIZE: u32 = 25;

/// The pause between two batches: 100 creations per second for 4 seconds. With every one of the 20 delivery slots
/// waiting 2 seconds on the slow endpoint the tenant would deliver 10 notifications per second between both
/// Subscriptions, so the 400 owed to the answering endpoint would take more than 40 seconds.
const BATCH_INTERVAL: Duration = Duration::from_millis(250);

/// How soon after the last batch the answering endpoint has been notified of every Tram.
const NOTIFIED_WITHIN: Duration = Duration::from_secs(5);

/// Pins: Under durable delivery, beside a Subscription whose endpoint answers each notification after 2 seconds, a
/// Subscription on the same Trams whose endpoint answers at once is notified of each of 400 Trams created at 100 per
/// second within 5 seconds of the last creation
/// Clause: 5.8.6
/// Tenant: private (the deliveries of one tenant share its 20 delivery slots, so the slow endpoint would hold back the
/// notifications of every other test in the shared tenant)
/// Recorded: Andromeda extension, not v1.9.1: durable notification delivery (notifications.delivery.mode durable on
/// broker-durable) with its default 20 slots, where a Subscription holding half of them is served only after the
/// Subscriptions with fewer notifications in flight
#[gtest]
#[tokio::test]
async fn a_ntf_dlv_9009_an_endpoint_that_answers_slowly_leaves_the_other_subscriptions_their_delivery_slots() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_durable = broker::client(BrokerRole::Durable)?;
    let tenant = broker::private_tenant(&broker_durable, &namespace).await?;
    let sink = NotificationSink::start().await?;
    sink.answer("slow", ResponseTemplate::new(204).set_delay(ANSWER_DELAY)).await?;
    let tram_type = namespace.entity_type("Tram")?;
    let subscription_slow_id = namespace.subscription_id("slow")?;
    let subscription_answering_id = namespace.subscription_id("answering")?;
    for (id, endpoint) in [(&subscription_slow_id, "slow"), (&subscription_answering_id, "answering")] {
        let subscription = Subscription::builder()
            .id(id.clone())
            .entities([tram_type.as_str()])
            .notification_triggers([NotificationTrigger::EntityCreated])
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(endpoint)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        tenant.subscriptions().create(&subscription).send().await?;
    }
    let mut created = Vec::new();
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
        tenant.entities().batch_create(&trams).send().await?.done()?;
        created.extend(trams.iter().map(|tram| tram.id.to_string()));
    }
    let received = timeout(NOTIFIED_WITHIN, sink.wait_for_at_least("answering", created.len())).await;
    expect_that!(received, ok(ok(len(ge(created.len())))));
    let mut notified = Vec::new();
    for notification in sink.received("answering").await? {
        notified.extend(notification.entities(Lossless::Normalized)?.data().iter().map(|entity| entity.id.to_string()));
    }
    notified.sort();
    created.sort();
    expect_that!(notified, eq(&created));
    tenant.subscriptions().delete(&subscription_slow_id).send().await?;
    tenant.subscriptions().delete(&subscription_answering_id).send().await?;
    Ok(())
}
