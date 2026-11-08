//! A broker that stops evaluates a backlog longer than one delivery deadline before it exits.

use andromeda_test_suite::{
    broker,
    broker_process::BrokerProcess,
    config::BrokerRole,
    namespace::Namespace,
    notification_sink::NotificationSink,
    tenant_set::TenantSet,
};
use futures::future;
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Input, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::time::Duration;
use wiremock::ResponseTemplate;

/// How many writes run at once, and how many entities each upserts.
const WRITES: usize = 3;
/// How many entities each write upserts.
const ENTITIES_PER_WRITE: usize = 15;
/// How long the endpoint takes to answer a notification: the changes left when the broker stops (the lane's 32 and the
/// queue's two) need about 17 s to send, more than a delivery deadline of 10 s and less than the shutdown drain of 30 s.
const ANSWER_DELAY: Duration = Duration::from_millis(500);

/// Pins: a Subscription on a type whose endpoint answers each notification after 500 ms, in a tenant of a broker
/// evaluating live through a queue of two changes and one lane, receives one notification for each of 45 entities
/// upserted by three concurrent batches (all answered 2xx), although the broker received SIGTERM right after the
/// writes, with the changes it still held needing longer to send than `notifications.delivery.request_timeout`
/// Clause: 5.8.6
/// Tenant: private (the subject is a whole broker process stopping with a tenant resident)
/// Recorded: Andromeda configuration, not v1.9.1: the broker evaluates live, buffers two changes beyond the 32 its
/// lane holds, makes a write wait when both are full, and stops a tenant's evaluation within
/// `notifications.evaluation.drain_timeout`
#[gtest]
#[tokio::test]
async fn p_dor_ntf_0007_every_change_of_a_live_subscription_is_notified_when_the_broker_receives_sigterm_with_a_backlog_past_the_delivery_deadline()
-> Result<()> {
    let namespace = Namespace::fresh();
    let process = BrokerProcess::of(BrokerRole::DormantLive)?;
    let live = broker::client(BrokerRole::DormantLive)?;
    let sink = NotificationSink::start().await?;
    sink.answer("slow", ResponseTemplate::new(200).set_delay(ANSWER_DELAY)).await?;
    let set = TenantSet::provision(&live, &namespace, 1, 1).await?;
    let member = set.member(0);
    let watched = namespace.entity_type("Watched")?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id("slow")?)
        .entities([watched.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("slow")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    member.client.subscriptions().create(&subscription).send().await?;
    let mut batches = Vec::with_capacity(WRITES);
    for write in 0..WRITES {
        let mut batch = Vec::with_capacity(ENTITIES_PER_WRITE);
        for entity in 0..ENTITIES_PER_WRITE {
            batch.push(
                Entity::<Input>::builder()
                    .id(namespace.urn("Watched", write * ENTITIES_PER_WRITE + entity)?)
                    .entity_type(watched.clone())
                    .property("reading", 1)
                    .build()?,
            );
        }
        batches.push(batch);
    }
    let entities = member.client.entities();
    let answered = future::join_all(batches.iter().map(|batch| entities.batch_upsert(batch).send())).await;
    for outcome in answered {
        outcome?;
    }
    process.terminate().await?;
    let notified = sink.wait_for_at_least("slow", WRITES * ENTITIES_PER_WRITE).await;
    process.start().await?;
    let notified = notified?;
    verify_that!(notified.len(), eq(WRITES * ENTITIES_PER_WRITE))
}
