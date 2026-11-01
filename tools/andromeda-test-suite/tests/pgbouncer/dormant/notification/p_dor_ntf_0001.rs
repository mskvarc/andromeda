//! A periodic Subscription keeps notifying after its tenant is evicted.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    eviction::evict_by_pressure,
    namespace::Namespace,
    notification_sink::NotificationSink,
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::time::Duration;

/// The window the notifications after the eviction are counted over: six intervals of 5 s.
const WINDOW: Duration = Duration::from_secs(30);

/// Pins: a Subscription with `timeInterval` 5 in a tenant the broker then evicts under the pressure of 40 other
/// tenants sends at least 4 notifications in the 30 s after the tenant was seen dormant
/// Clause: 5.8.6
/// Tenant: private (the subject is a whole tenant leaving the runtime cache)
/// Recorded: Andromeda configuration, not v1.9.1: the broker keeps 32 tenants resident and loads an evicted tenant
/// again for its due work 3 s after the eviction at the earliest, sweeping every 5 s; 4 of the window's 6 intervals
/// leave room for both
#[gtest]
#[tokio::test]
async fn p_dor_ntf_0001_a_periodic_subscription_keeps_notifying_after_its_tenant_is_evicted() -> Result<()> {
    let namespace = Namespace::fresh();
    let dormant = broker::client(BrokerRole::Dormant)?;
    let sink = NotificationSink::start().await?;
    let set = TenantSet::provision(&dormant, &namespace, 1, 1).await?;
    let member = set.member(0);
    let subscription = Subscription::builder()
        .id(namespace.subscription_id("periodic")?)
        .entities([namespace.entity_type("Device")?.as_str()])
        .time_interval(5)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("periodic")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    member.client.subscriptions().create(&subscription).send().await?;
    evict_by_pressure(&dormant, &member.tenant).await?;
    let before = sink.received("periodic").await?.len();
    let after = sink.count_after("periodic", WINDOW).await?;
    verify_that!(after.saturating_sub(before), ge(4))
}
