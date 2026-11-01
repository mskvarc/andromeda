//! Durable delivery through the pooler notifies each change exactly once.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    load_outcome::LoadOutcome,
    namespace::Namespace,
    notification_sink::NotificationSink,
    tenant_set::TenantSet,
};
use futures::future::join_all;
use googletest::prelude::*;
use ngsi_ld::{
    model::{EntityFragment, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::time::Duration;

/// The tenants, each with a Subscription of its own.
const TENANTS: usize = 16;

/// The changes of each tenant's Device.
const CHANGES: i32 = 50;

/// How long the notifications must stay at their count once it is reached.
const QUIET: Duration = Duration::from_secs(8);

/// Pins: in each of 16 tenants of a broker delivering durably behind a transaction pooler, a Subscription to the
/// tenant's Device type is notified exactly 50 times for 50 Update Attributes of distinct values, each answered with a
/// success, and no more in the 8 s after
/// Clause: 5.8.6
/// Tenant: private (the subject is the delivery queues of whole tenant databases)
/// Recorded: Andromeda deployment, not v1.9.1: the broker delivers durably and serves its requests through a
/// transaction-pooling `PgBouncer`
#[gtest]
#[tokio::test]
async fn p_sem_ntf_0001_durable_delivery_through_the_pooler_notifies_each_change_exactly_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let pooled = broker::quiet_client(BrokerRole::PooledDurable)?;
    let sink = NotificationSink::start().await?;
    let set = TenantSet::provision(&pooled, &namespace, TENANTS, TENANTS).await?;
    let device_type = namespace.entity_type("Device")?;
    for (index, member) in set.members().enumerate() {
        let subscription = Subscription::builder()
            .id(namespace.subscription_id(index)?)
            .entities([device_type.as_str()])
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(index)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        member.client.subscriptions().create(&subscription).send().await?;
    }
    for value in 0..CHANGES {
        let change = EntityFragment::builder().property("reading", 100 + value).build()?;
        let changed = join_all(
            set.members()
                .map(|member| async { LoadOutcome::of(&member.client.with_responses().entities().update_attributes(set.device(), &change).send().await) }),
        )
        .await;
        expect_that!(changed, each(predicate(|answer: &LoadOutcome| answer.is_success())));
    }
    let changes = usize::try_from(CHANGES)?;
    for index in 0..set.len() {
        sink.wait_for_at_least(index, changes).await?;
        expect_that!(sink.count_after(index, QUIET).await?, eq(changes), "tenant {index}");
    }
    Ok(())
}
