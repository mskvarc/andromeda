//! A tenant with live changes still waiting is not unloaded to make room until they are notified.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    eviction::evict_by_reading,
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
/// How long the endpoint takes to answer a notification: 45 changes outrun an evaluation queue of two and a lane of
/// 32, so the writes wait for room, and the changes left when the tenant is evicted need several seconds to send.
const ANSWER_DELAY: Duration = Duration::from_millis(250);

/// Pins: when the cache of three needs room right after three concurrent batches of 15 entities were answered 2xx, the
/// tenant of a live Subscription whose endpoint answers each notification after 250 ms is unloaded only once all 45
/// notifications have arrived, and no notification is lost
/// Clause: 5.8.6
/// Tenant: private (the subject is a whole tenant leaving the runtime cache)
/// Recorded: Andromeda configuration, not v1.9.1: the broker evaluates live, buffers two changes beyond the 32 its
/// lane holds, makes a write wait when both are full, and unloads a tenant holding changes only after every tenant
/// holding none
#[gtest]
#[tokio::test]
async fn p_dor_ntf_0005_a_tenant_with_live_changes_waiting_is_unloaded_only_after_they_are_notified() -> Result<()> {
    let namespace = Namespace::fresh();
    let pressure = Namespace::fresh();
    let live = broker::client(BrokerRole::DormantLive)?;
    let sink = NotificationSink::start().await?;
    sink.answer("slow", ResponseTemplate::new(200).set_delay(ANSWER_DELAY)).await?;
    let others = TenantSet::provision(&live, &pressure, 5, 5).await?;
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
    evict_by_reading(&live, &member.tenant, &others).await?;
    let notified = sink.received("slow").await?;
    verify_that!(notified.len(), eq(WRITES * ENTITIES_PER_WRITE))
}
